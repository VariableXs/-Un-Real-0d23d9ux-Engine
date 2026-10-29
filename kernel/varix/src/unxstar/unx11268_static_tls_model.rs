//! UNX-F11268 musl 静态 TLS 模型消费（0x1f440 段）
//!
//! 纯功能模型：musl 静态链接件的 TLS 走 Variant II 布局——TCB 位于静态
//! TLS 映像末尾，TP（%fs 基址）指向 TCB，因此**所有模块相对 TP 的偏移都是
//! 负值**。本模块把「模块登记 → 布局定锚 → 成员地址反解」三段串成一条账：
//! 登记面校验模块号、对齐、tdata/tbss 尺寸与重号；布局面按登记序逐块对齐
//! 推进，算出映像总长与 TP 锚点；消费面按（模块号，成员偏移）反解 TP 相对
//! 偏移与绝对地址，越界即拒。
//!
//! 规则面：对齐必须是 2 的幂且不超过一页（4096）；tdata+tbss 必须非空且不
//! 超过 1 MiB；模块号唯一且非零；布局只在登记完成后做一次，定锚即封闭登记
//! 面。`__set_thread_area` 在此折叠为「TP 单值锚定」——布局一旦定锚，只接受
//! 与锚点逐位相同的 TP，否则判语义不一致；重放（replay）必须复现同一指纹。
//!
//! 拒止面：TL_EID（模块号为零）、TL_EALIGN（非 2 的幂 / 超页）、TL_ESIZE
//! （空块 / 超上限）、TL_EDUP（重号）、TL_EFULL（席位满）、TL_ELID（未布局
//! 即消费）、TL_EOFF（成员偏移越过 tdata+tbss）、TL_EFROZEN（定锚后动账）、
//! TL_EEMPTY（空映像布局）、TL_EMISMATCH（TP 不一致 / 重放指纹不一致）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x1f440 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f440;

/// 模块号为零（无主模块）
pub const TL_EID: u32 = SEG_BASE + 0x01;
/// 对齐非 2 的幂或超过一页
pub const TL_EALIGN: u32 = SEG_BASE + 0x02;
/// tdata+tbss 为空或超上限
pub const TL_ESIZE: u32 = SEG_BASE + 0x03;
/// 模块号重号
pub const TL_EDUP: u32 = SEG_BASE + 0x04;
/// 模块席位已满
pub const TL_EFULL: u32 = SEG_BASE + 0x05;
/// 未布局即消费地址
pub const TL_ELID: u32 = SEG_BASE + 0x06;
/// 成员偏移越过 tdata+tbss
pub const TL_EOFF: u32 = SEG_BASE + 0x07;
/// 定锚 / 冻结后动账
pub const TL_EFROZEN: u32 = SEG_BASE + 0x08;
/// 空映像不得布局
pub const TL_EEMPTY: u32 = SEG_BASE + 0x09;
/// TP 不一致或重放指纹不一致
pub const TL_EMISMATCH: u32 = SEG_BASE + 0x0a;

/// 模块席位（静态 TLS 常见 5~8 个，留余量）。
pub const MAX_MODULES: usize = 12;
/// 对齐上限（一页）。
pub const ALIGN_MAX: u32 = 4096;
/// 单模块 tdata+tbss 上限（1 MiB）。
pub const SIZE_MAX: u32 = 0x10_0000;
/// TCB 尺寸（x86-64 musl：pthread 结构体自 TP 起，前 64 字节字段齐备）。
pub const TCB_SIZE: u32 = 64;
/// TCB 对齐。
pub const TCB_ALIGN: u32 = 64;
/// 静态 TLS 映像基址（Variant II 由内核在 exec 时铺定）。
pub const TLS_BASE: u64 = 0x7f00_0000_0000;
/// TLS 变体号：II（TP 指向块尾 TCB，模块偏移为负）。
pub const TLS_VARIANT: u8 = 2;
/// 布局推进的模块序即登记序（musl 静态链接按 link 顺序铺块）。
pub const LAYOUT_IN_ORDER: bool = true;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 2 的幂判定（零不算）。
#[inline]
pub fn is_pow2(v: u32) -> bool {
    v != 0 && (v & v.wrapping_sub(1)) == 0
}

/// 向上对齐（调用方保证 a 为 2 的幂；a<=1 时原值返回）。
#[inline]
pub fn align_up(v: u32, a: u32) -> u32 {
    if a <= 1 {
        return v;
    }
    v.wrapping_add(a.wrapping_sub(1)) & !(a.wrapping_sub(1))
}

/// 登记用模块描述：模块号 / tdata 长 / tbss 长 / 对齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TlsModule {
    pub id: u16,
    pub tdata: u32,
    pub tbss: u32,
    pub align: u16,
}

/// 落位后的模块槽：登记值 + 布局偏移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub id: u16,
    pub tdata: u32,
    pub tbss: u32,
    pub align: u16,
    pub offset: u32,
}

impl Slot {
    /// 本模块在映像内占的字节数（tdata 在前、tbss 紧随其后）。
    pub fn size(&self) -> u32 {
        self.tdata.wrapping_add(self.tbss)
    }
}

/// 静态 TLS 映像账：登记槽 + 布局结果 + 锚点 + 指纹。
#[derive(Clone, Debug)]
pub struct TlsImage {
    slots: [Option<Slot>; MAX_MODULES],
    used: usize,
    laid: bool,
    image_total: u32,
    tp: u64,
    root: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
    pub lookups: u64,
    pub tp_sets: u64,
    pub replays: u64,
}

impl TlsImage {
    /// 空映像。
    pub fn new() -> TlsImage {
        TlsImage {
            slots: [None; MAX_MODULES],
            used: 0,
            laid: false,
            image_total: 0,
            tp: 0,
            root: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
            lookups: 0,
            tp_sets: 0,
            replays: 0,
        }
    }

    /// 已登记模块数。
    pub fn used(&self) -> usize {
        self.used
    }

    /// 是否已布局定锚。
    pub fn is_laid(&self) -> bool {
        self.laid
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 映像总长（已含尾部对齐，不含 TCB）。
    pub fn image_total(&self) -> u32 {
        self.image_total
    }

    /// TP 绝对锚点。
    pub fn tp(&self) -> u64 {
        self.tp
    }

    /// 第 i 个登记槽（按登记序）。
    pub fn slot(&self, i: usize) -> Option<Slot> {
        match self.slots.get(i) {
            Some(v) => *v,
            None => None,
        }
    }

    /// 第 i 个登记槽的布局偏移。
    pub fn offset_of(&self, i: usize) -> Option<u32> {
        match self.slot(i) {
            Some(s) => Some(s.offset),
            None => None,
        }
    }

    /// 按模块号查登记槽下标。
    pub fn find(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.used {
            match self.slots.get(i) {
                Some(Some(s)) if s.id == id => return Some(i),
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 已登记模块的 tdata 总长。
    pub fn tdata_bytes(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.used {
            match self.slots.get(i) {
                Some(Some(s)) => n = n.wrapping_add(s.tdata),
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 已登记模块的 tbss 总长。
    pub fn tbss_bytes(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.used {
            match self.slots.get(i) {
                Some(Some(s)) => n = n.wrapping_add(s.tbss),
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 记一次拒止并回传错误码。
    fn note(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记一个模块。校验序：定锚/冻结 → 模块号 → 对齐 → 尺寸 → 重号 → 席位。
    pub fn admit(&mut self, m: TlsModule) -> Result<u64, u32> {
        if self.frozen || self.laid {
            return Err(self.note(TL_EFROZEN));
        }
        if m.id == 0 {
            return Err(self.note(TL_EID));
        }
        let a = m.align as u32;
        if !is_pow2(a) || a > ALIGN_MAX {
            return Err(self.note(TL_EALIGN));
        }
        let size = m.tdata.wrapping_add(m.tbss);
        if size == 0 || size > SIZE_MAX {
            return Err(self.note(TL_ESIZE));
        }
        if self.find(m.id).is_some() {
            return Err(self.note(TL_EDUP));
        }
        if self.used >= MAX_MODULES {
            return Err(self.note(TL_EFULL));
        }
        let slot = Slot {
            id: m.id,
            tdata: m.tdata,
            tbss: m.tbss,
            align: m.align,
            offset: 0,
        };
        match self.slots.get_mut(self.used) {
            Some(cell) => *cell = Some(slot),
            None => return Err(self.note(TL_EFULL)),
        }
        self.used = self.used.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 布局定锚：登记序逐块对齐推进，尾部按 TCB 对齐取整，TP = 基址 + 总长 + TCB。
    /// 二次布局幂等（回同一指纹），空映像拒。
    pub fn layout(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.note(TL_EFROZEN));
        }
        if self.used == 0 {
            return Err(self.note(TL_EEMPTY));
        }
        if self.laid {
            return match self.root {
                Some(r) => Ok(r),
                None => Ok(self.fold()),
            };
        }
        let mut cursor = 0u32;
        let mut i = 0usize;
        while i < self.used {
            let mut s = match self.slots.get(i) {
                Some(Some(v)) => *v,
                _ => break,
            };
            let off = align_up(cursor, s.align as u32);
            // tddata 自身的起点按对齐落位；tdata 内成员对 16 字节的向量槽
            // 由链接器保证，这里不重复断言。
            s.offset = off;
            cursor = off.wrapping_add(s.tdata).wrapping_add(s.tbss);
            match self.slots.get_mut(i) {
                Some(cell) => *cell = Some(s),
                None => break,
            }
            i = i.wrapping_add(1);
        }
        self.image_total = align_up(cursor, TCB_ALIGN);
        self.tp = TLS_BASE
            .wrapping_add(self.image_total as u64)
            .wrapping_add(TCB_SIZE as u64);
        self.laid = true;
        let v = self.fold();
        self.root = Some(v);
        Ok(v)
    }

    /// 成员字号（tdata+tbss），未知模块回 None。
    pub fn module_size(&self, id: u16) -> Option<u32> {
        match self.find(id) {
            Some(i) => self.slots.get(i).and_then(|v| *v).map(|s| s.size()),
            None => None,
        }
    }

    /// 成员绝对地址：TLS_BASE + 模块偏移 + 成员偏移。
    pub fn member_addr(&mut self, id: u16, off: u32) -> Result<u64, u32> {
        if !self.laid {
            return Err(self.note(TL_ELID));
        }
        let i = match self.find(id) {
            Some(v) => v,
            None => return Err(self.note(TL_EID)),
        };
        let s = match self.slots.get(i) {
            Some(Some(v)) => *v,
            _ => return Err(self.note(TL_EID)),
        };
        if off >= s.size() {
            return Err(self.note(TL_EOFF));
        }
        self.lookups = self.lookups.wrapping_add(1);
        Ok(TLS_BASE
            .wrapping_add(s.offset as u64)
            .wrapping_add(off as u64))
    }

    /// Variant II 的 TP 相对偏移（必为负）：(模块偏移 + 成员偏移) - (总长 + TCB)。
    pub fn tp_rel(&mut self, id: u16, off: u32) -> Result<i64, u32> {
        let abs = match self.member_addr(id, off) {
            Ok(v) => v,
            Err(c) => return Err(c),
        };
        if abs >= self.tp {
            return Err(self.note(TL_EMISMATCH));
        }
        Ok((abs as i64).wrapping_sub(self.tp as i64))
    }

    /// `__set_thread_area` 语义：只接受与布局锚点逐位相同的 TP，且须 TCB 对齐。
    pub fn set_thread_area(&mut self, tp: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.note(TL_EFROZEN));
        }
        if !self.laid {
            return Err(self.note(TL_ELID));
        }
        if tp != self.tp {
            return Err(self.note(TL_EMISMATCH));
        }
        if tp & (TCB_ALIGN as u64).wrapping_sub(1) != 0 {
            return Err(self.note(TL_EALIGN));
        }
        self.tp_sets = self.tp_sets.wrapping_add(1);
        Ok(self.fold())
    }

    /// 布局指纹（FNV-1a）：只看布局面，不含记账计数，故重放可比。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ TLS_VARIANT as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.used as u64).wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < self.used {
            match self.slots.get(i) {
                Some(Some(s)) => {
                    h = (h ^ s.id as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ s.offset as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ s.tdata as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ s.tbss as u64).wrapping_mul(FNV_PRIME);
                    h = (h ^ s.align as u64).wrapping_mul(FNV_PRIME);
                }
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        h = (h ^ self.image_total as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.tp).wrapping_mul(FNV_PRIME);
        h = (h ^ TCB_SIZE as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 冻结：锁定布局指纹，幂等；未布局不得冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return match self.root {
                Some(r) => Ok(r),
                None => Ok(self.fold()),
            };
        }
        if !self.laid {
            return Err(self.note(TL_ELID));
        }
        let v = match self.root {
            Some(r) => r,
            None => self.fold(),
        };
        self.root = Some(v);
        self.frozen = true;
        Ok(v)
    }

    /// 定锚值。
    pub fn root_value(&self) -> Option<u64> {
        self.root
    }

    /// 重放：从登记面重算布局，指纹必须与原布局逐位一致。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.laid {
            return Err(self.note(TL_ELID));
        }
        let base = self.fold();
        let mut probe = self.clone();
        probe.laid = false;
        probe.image_total = 0;
        probe.tp = 0;
        probe.root = None;
        probe.frozen = false;
        match probe.layout() {
            Ok(v) => {
                if v != base {
                    return Err(self.note(TL_EMISMATCH));
                }
            }
            Err(_) => return Err(self.note(TL_EMISMATCH)),
        }
        self.replays = self.replays.wrapping_add(1);
        Ok(base)
    }
}

/// 基准映像：四个模块，覆盖「tdata+tbss / 纯 tdata / 小对齐 / 页对齐纯 tbss」。
/// 预期布局：偏移 0x000 / 0x0c0 / 0x1c0 / 0x1000，总长 0x1200，TP 偏移 -0x1240。
pub fn build_image() -> (TlsImage, bool) {
    let spec = [
        TlsModule { id: 1, tdata: 0x40, tbss: 0x80, align: 8 },
        TlsModule { id: 2, tdata: 0x100, tbss: 0, align: 64 },
        TlsModule { id: 3, tdata: 0x8, tbss: 0x8, align: 16 },
        TlsModule { id: 4, tdata: 0, tbss: 0x200, align: 4096 },
    ];
    let mut img = TlsImage::new();
    let mut i = 0usize;
    while i < spec.len() {
        match spec.get(i) {
            Some(m) => {
                if img.admit(*m).is_err() {
                    return (img, false);
                }
            }
            None => break,
        }
        i = i.wrapping_add(1);
    }
    let laid = img.layout().is_ok();
    let frozen = img.freeze().is_ok();
    (img, laid && frozen)
}

/// 四类注入：对齐非法 / 尺寸非法 / 重号 / 定锚后动账。
pub fn inject_four() -> [bool; 4] {
    // ① 对齐非法：非 2 的幂、零、超页三路
    let mut a0 = TlsImage::new();
    let r0 = a0.admit(TlsModule { id: 1, tdata: 8, tbss: 0, align: 3 }).err() == Some(TL_EALIGN)
        && a0.admit(TlsModule { id: 1, tdata: 8, tbss: 0, align: 0 }).err() == Some(TL_EALIGN)
        && a0.admit(TlsModule { id: 1, tdata: 8, tbss: 0, align: 8192 }).err() == Some(TL_EALIGN)
        && a0.used() == 0;

    // ② 尺寸非法：空块与超上限
    let mut a1 = TlsImage::new();
    let r1 = a1.admit(TlsModule { id: 1, tdata: 0, tbss: 0, align: 8 }).err() == Some(TL_ESIZE)
        && a1
            .admit(TlsModule { id: 2, tdata: SIZE_MAX, tbss: 1, align: 8 })
            .err()
            == Some(TL_ESIZE)
        && a1.used() == 0;

    // ③ 重号：同模块号二次登记
    let mut a2 = TlsImage::new();
    let s2 = a2.admit(TlsModule { id: 7, tdata: 0x10, tbss: 0x10, align: 16 }).is_ok();
    let r2 = s2
        && a2.admit(TlsModule { id: 7, tdata: 0x20, tbss: 0, align: 8 }).err() == Some(TL_EDUP)
        && a2.used() == 1;

    // ④ 定锚后动账：布局后登记与二次布局都拒
    let mut a3 = TlsImage::new();
    let s3 = a3.admit(TlsModule { id: 1, tdata: 0x10, tbss: 0x10, align: 8 }).is_ok();
    let l3 = a3.layout().is_ok();
    let r3 = s3
        && l3
        && a3.admit(TlsModule { id: 2, tdata: 8, tbss: 0, align: 8 }).err() == Some(TL_EFROZEN)
        && a3.set_thread_area(TLS_BASE).err() == Some(TL_EMISMATCH);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11268 静态 TLS（TP 偏移直取）。
pub fn run_unx11268_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11268.static_tls_model");
    let (mut img, ok) = build_image();

    // TL1 登记定容
    s.add_dyn(
        "TL1-四模块登记",
        ok && img.used() == 4 && img.accepts == 4 && img.rejects == 0,
        &format!("used={} acc={} rej={}", img.used(), img.accepts, img.rejects),
    );

    // TL2 布局推进：0 / 0xc0 / 0x1c0 / 0x1000，总长 0x1200
    let o0 = img.offset_of(0).unwrap_or(0);
    let o1 = img.offset_of(1).unwrap_or(0);
    let o2 = img.offset_of(2).unwrap_or(0);
    let o3 = img.offset_of(3).unwrap_or(0);
    s.add_dyn(
        "TL2-布局推进",
        ok && o0 == 0 && o1 == 0xc0 && o2 == 0x1c0 && o3 == 0x1000
            && img.image_total() == 0x1200,
        &format!("off={:#x}/{:#x}/{:#x}/{:#x} tot={:#x}", o0, o1, o2, o3, img.image_total()),
    );

    // TL3 对齐保持：每槽偏移对自身对齐取模为零
    let mut aligned = true;
    let mut i = 0usize;
    while i < img.used() {
        match img.slot(i) {
            Some(sl) => {
                if sl.offset % (sl.align as u32) != 0 {
                    aligned = false;
                }
            }
            None => aligned = false,
        }
        i = i.wrapping_add(1);
    }
    s.add("TL3-对齐保持", ok && aligned, "每模块起点落在其声明对齐上");

    // TL4 TP 锚定：基址 + 总长 + TCB，且总长按 TCB 对齐
    let tp = img.tp();
    s.add_dyn(
        "TL4-TP锚定",
        tp == TLS_BASE + 0x1200 + TCB_SIZE as u64 && img.image_total() % TCB_ALIGN == 0,
        &format!("tp={:#x} tot={:#x}", tp, img.image_total()),
    );

    // TL5 Variant II 负偏移：首成员与末成员都在 TP 之下
    let n0 = img.tp_rel(1, 0).unwrap_or(0);
    let n1 = img.tp_rel(4, 0x1ff).unwrap_or(0);
    s.add_dyn(
        "TL5-VariantII负偏移",
        n0 == -0x1240 && n1 < 0 && n0 < 0,
        &format!("rel1={} rel4={}", n0, n1),
    );

    // TL6 成员反解：绝对地址 = 基址 + 模块偏移 + 成员偏移
    let a1 = img.member_addr(2, 0x10).unwrap_or(0);
    let a2 = img.member_addr(4, 0x1ff).unwrap_or(0);
    s.add_dyn(
        "TL6-成员反解",
        a1 == TLS_BASE + 0xc0 + 0x10 && a2 == TLS_BASE + 0x1000 + 0x1ff,
        &format!("a1={:#x} a2={:#x}", a1, a2),
    );

    // TL7 字节账：tdata 0x148 / tbss 0x288
    s.add_dyn(
        "TL7-字节账",
        img.tdata_bytes() == 0x148 && img.tbss_bytes() == 0x288,
        &format!("td={:#x} tb={:#x}", img.tdata_bytes(), img.tbss_bytes()),
    );

    // TL8 重号拒止
    let mut d = TlsImage::new();
    let d0 = d.admit(TlsModule { id: 3, tdata: 8, tbss: 8, align: 8 }).is_ok();
    s.add(
        "TL8-重号拒止",
        d0 && d.admit(TlsModule { id: 3, tdata: 16, tbss: 0, align: 8 }).err() == Some(TL_EDUP)
            && d.used() == 1,
        "同模块号二次登记拒止且席位不减",
    );

    // TL9 对齐拒止
    let mut q = TlsImage::new();
    let qa = q.admit(TlsModule { id: 1, tdata: 8, tbss: 0, align: 6 }).err() == Some(TL_EALIGN);
    let qb = q.admit(TlsModule { id: 1, tdata: 8, tbss: 0, align: 8192 }).err() == Some(TL_EALIGN);
    s.add(
        "TL9-对齐拒止",
        qa && qb && q.used() == 0 && q.rejects == 2,
        "非 2 的幂与超页对齐两路俱拒",
    );

    // TL10 尺寸拒止
    let mut z = TlsImage::new();
    let za = z.admit(TlsModule { id: 1, tdata: 0, tbss: 0, align: 8 }).err() == Some(TL_ESIZE);
    let zb = z
        .admit(TlsModule { id: 1, tdata: SIZE_MAX, tbss: 2, align: 8 })
        .err()
        == Some(TL_ESIZE);
    s.add(
        "TL10-尺寸拒止",
        za && zb && z.used() == 0,
        "空块与超上限两路俱拒",
    );

    // TL11 越界/未知/未布局三路拒止
    let mut o = TlsImage::new();
    let pre = o.member_addr(1, 0).err() == Some(TL_ELID);
    let _ = o.admit(TlsModule { id: 1, tdata: 0x10, tbss: 0, align: 8 });
    let laid = o.layout().is_ok();
    let over = o.member_addr(1, 0x10).err() == Some(TL_EOFF);
    let unk = o.member_addr(9, 0).err() == Some(TL_EID);
    s.add_dyn(
        "TL11-越界反解拒止",
        pre && laid && over && unk && o.rejects == 3,
        &format!("pre={} over={} unk={} rej={}", pre as u8, over as u8, unk as u8, o.rejects),
    );

    // TL12 冻结幂等 + 定锚后拒
    let f1 = img.freeze().unwrap_or(0);
    let f2 = img.freeze().unwrap_or(0);
    let closed = img
        .admit(TlsModule { id: 9, tdata: 8, tbss: 0, align: 8 })
        .err()
        == Some(TL_EFROZEN)
        && img.layout().err() == Some(TL_EFROZEN);
    s.add_dyn(
        "TL12-冻结幂等",
        f1 == f2 && f1 != 0 && img.root_value() == Some(f1) && closed,
        &format!("f1={:#x} f2={:#x} closed={}", f1, f2, closed as u8),
    );

    // TL13 注入四拒 + 重放同指纹
    let hits = inject_four();
    let (mut re, ok2) = build_image();
    let rep = re.replay().unwrap_or(0);
    s.add_dyn(
        "TL13-注入四拒与重放",
        hits[0] && hits[1] && hits[2] && hits[3] && ok2 && rep == re.fold() && rep != 0,
        &format!(
            "inj={}{}{}{} rep={:#x} replays={}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8, rep, re.replays
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11268_tls_flow() {
        let (mut img, ok) = build_image();
        assert!(ok);
        assert_eq!(img.used(), 4);
        assert!(img.is_laid() && img.is_frozen());
        assert_eq!(img.offset_of(0), Some(0));
        assert_eq!(img.offset_of(1), Some(0xc0));
        assert_eq!(img.offset_of(2), Some(0x1c0));
        assert_eq!(img.offset_of(3), Some(0x1000));
        assert_eq!(img.image_total(), 0x1200);
        assert_eq!(img.tp(), TLS_BASE + 0x1240);
        assert_eq!(img.tp_rel(1, 0), Ok(-0x1240));
        assert_eq!(img.member_addr(3, 7), Ok(TLS_BASE + 0x1c0 + 7));
        let root = img.root_value();
        assert!(root.is_some());
        assert_eq!(img.freeze().ok(), root);
        assert_eq!(img.replay().ok(), Some(img.fold()));
        assert_eq!(img.replays, 1);
    }

    #[test]
    fn f11268_tls_gates() {
        let mut t = TlsImage::new();
        assert!(t.admit(TlsModule { id: 5, tdata: 0x20, tbss: 0x20, align: 8 }).is_ok());
        assert_eq!(t.admit(TlsModule { id: 0, tdata: 8, tbss: 0, align: 8 }), Err(TL_EID));
        assert_eq!(t.admit(TlsModule { id: 9, tdata: 0, tbss: 0, align: 8 }), Err(TL_ESIZE));
        assert_eq!(t.admit(TlsModule { id: 9, tdata: 8, tbss: 0, align: 6 }), Err(TL_EALIGN));
        assert_eq!(t.admit(TlsModule { id: 5, tdata: 8, tbss: 0, align: 8 }), Err(TL_EDUP));
        assert_eq!(t.admit(TlsModule { id: 9, tdata: 8, tbss: 0, align: 8192 }), Err(TL_EALIGN));
        assert_eq!(
            t.admit(TlsModule { id: 9, tdata: SIZE_MAX, tbss: 1, align: 8 }),
            Err(TL_ESIZE)
        );
        assert_eq!(t.accepts, 1);
        assert_eq!(t.rejects, 6);
        assert!(t.layout().is_ok());
        assert_eq!(t.admit(TlsModule { id: 6, tdata: 8, tbss: 0, align: 8 }), Err(TL_EFROZEN));
        assert_eq!(t.rejects, 7);
        assert_eq!(t.member_addr(5, 0x40), Err(TL_EOFF));
        assert_eq!(t.member_addr(0x77, 0), Err(TL_EID));
    }

    #[test]
    fn f11268_tls_rules() {
        let (mut img, _) = build_image();
        let mut i = 0usize;
        let mut prev = 0u32;
        while i < img.used() {
            let sl = img.slot(i).expect("slot");
            assert_eq!(sl.offset % (sl.align as u32), 0);
            assert!(sl.offset >= prev);
            prev = sl.offset;
            i += 1;
        }
        assert_eq!(img.tdata_bytes(), 0x148);
        assert_eq!(img.tbss_bytes(), 0x288);
        assert_eq!(img.module_size(4), Some(0x200));
        assert_eq!(img.module_size(8), None);
        // 每个模块的末字节都在 TP 之下（Variant II）
        let mut k = 0usize;
        while k < img.used() {
            let sl = img.slot(k).expect("slot");
            let rel = img.tp_rel(sl.id, sl.size() - 1).expect("rel");
            assert!(rel < 0);
            k += 1;
        }
        // 空映像拒布局
        let mut e = TlsImage::new();
        assert_eq!(e.layout(), Err(TL_EEMPTY));
        // TP 只认锚点
        let mut f = TlsImage::new();
        assert!(f.admit(TlsModule { id: 1, tdata: 0x10, tbss: 0, align: 16 }).is_ok());
        assert!(f.layout().is_ok());
        assert_eq!(f.set_thread_area(f.tp() ^ 0x40), Err(TL_EMISMATCH));
        assert!(f.set_thread_area(f.tp()).is_ok());
        assert_eq!(f.tp_sets, 1);
    }

    #[test]
    fn f11268_tls_injection() {
        let hits = inject_four();
        assert!(hits[0] && hits[1] && hits[2] && hits[3]);
    }

    #[test]
    fn f11268_checks_green() {
        let cs = run_unx11268_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
    }
}
