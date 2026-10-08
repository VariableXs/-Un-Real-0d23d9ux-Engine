//! 任务15 · 映像装载原语抽象 + ELF 装载器（ELF/PE 共用）。
//!
//! 装载引擎对镜像格式与内存模型**双零知识**：它只消费两样东西——
//! - [`ImageFormat`]：格式适配器（ELF 一份、未来 PE/Wine 层一份），
//!   把"这个可执行文件有哪些段"翻译成统一的 [`SegmentView`]；
//! - [`UserMapper`]：页表+帧侧（目标真表+PMM / 宿主假表+假内存），
//!   帧分配、用户映射、帧写入、TLB 失效、摘叶与帧归还全在此抽象之后。
//!
//! 任务14 的血泪修复全部沉淀在引擎里，成为 ELF/PE 共用原语：
//! - **同页多段共享一帧**：`.rodata@0x401000` 与 `.data@0x401088` 同页时
//!   各自分配新帧并整体覆写 PTE，W 帧顶掉 R 帧，MSG 读出全零；
//! - **页内真实偏移拷贝**：段可以从页中间开始；
//! - **flags 并集**：任一段要 W 即 W、任一段要 NX 即 NX；
//! - **BSS 零语义**：帧先零化、文件内容按精确区间落位，filesz 之外
//!   天然为零——同页多段时每个段的拷贝互不越界，零区不被污染。
//!
//! 退出回收走同一抽象：装载返回逐页账本 [`LoadedImage::pages`]，
//! [`release_pages`] 摘叶、核对原帧、归还帧——曾经的 32 项定容数组
//! 改为 alloc::vec::Vec，大镜像无隐形上限。

use alloc::vec::Vec;

/// 4KiB 页面常量（与 mem::paging 同值；引擎自带避免反向依赖）。
pub const PAGE: u64 = 4096;

/// 物理帧地址掩码（x86-64 物理地址 ≤ 52 位，页内零）。
const P_ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;

/// 格式中立的段视图——ELF 的 PT_LOAD 与 PE 的 section 都翻译成它。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentView {
    pub vaddr: u64,
    /// 段文件内容在 blob 内的字节偏移。
    pub offset: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub writable: bool,
    pub executable: bool,
}

/// 映像格式适配器：ELF / PE 各一份实现，引擎零格式知识。
pub trait ImageFormat {
    fn entry(&self) -> u64;
    /// 段 offset 所指的字节源（ELF/PE 的整个文件映像）。
    fn blob(&self) -> &[u8];
    fn for_each_segment(&self, f: &mut dyn FnMut(SegmentView));
}

/// 装载引擎看到的页表+帧侧。目标真表与宿主假表各一份实现。
pub trait UserMapper {
    /// 分配并零化一个 4KiB 帧；None = 内存耗尽。
    fn alloc_zero_frame(&mut self) -> Option<u64>;
    /// 建/改 4KiB 用户叶（实现方负责 P_USER 置位与用户半区校验）。
    fn map_user_frame(&mut self, va: u64, phys: u64, w: bool, nx: bool) -> bool;
    /// 向帧内写入字节（phys 页基址 + off 页内偏移）；引擎唯一的写帧通道，
    /// 由实现方决定物理内存怎么摸（目标=HHDM，宿主=测试内存）。
    fn write_frame_bytes(&mut self, phys: u64, off: u64, data: &[u8]);
    fn flush(&mut self, va: u64);
    /// 摘除 4KiB 用户叶；返回原物理帧（None = 未映射/大叶/内核半区）。
    fn unmap_user(&mut self, va: u64) -> Option<u64>;
    /// 归还一个帧（目标态 free_order；宿主记账）。
    fn free_frame(&mut self, phys: u64);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// 段声明的内容越过 blob 边界——格式适配器或镜像损坏。
    BadBlob,
    /// 映射被拒（内核半区 / 页表建不起来）。
    MapFailed,
    /// 物理帧耗尽。
    OutOfMemory,
}

/// 装载结果：入口、栈顶与逐页账本（退出回收的唯一凭据）。
#[derive(Debug)]
pub struct LoadedImage {
    pub entry: u64,
    pub stack_top: u64,
    /// (页基址 va, 物理帧) —— 段页 + 栈页，按映射顺序。
    pub pages: Vec<(u64, u64)>,
    pub frames_used: u64,
}

/// 装载引擎：页账本逐页落位。任何一步失败都回滚已映射页（摘叶+归还帧），
/// 不留半个进程的地址空间。
pub fn load_into(
    src: &dyn ImageFormat,
    mp: &mut dyn UserMapper,
    stack_pages: u64,
    stack_top: u64,
) -> Result<LoadedImage, LoadError> {
    let blob = src.blob();
    // 页账本：(页基址, 帧, writable, nx)。同页多段共享一帧 + flags 并集。
    let mut ledger: Vec<(u64, u64, bool, bool)> = Vec::new();
    let mut pages: Vec<(u64, u64)> = Vec::new();
    let mut load_err: Option<LoadError> = None;

    src.for_each_segment(&mut |seg| {
        if load_err.is_some() {
            return;
        }
        let pages_of_seg = seg.memsz.div_ceil(PAGE).max(1);
        for k in 0..pages_of_seg {
            let va = seg.vaddr + k * PAGE;
            let page_va = va & !0xFFF;
            let in_page = va & 0xFFF;
            // 该页是否已有帧（同页多段共享，绝不重复分配）。
            let mut new_frame = false;
            let (phys, w, nx) = match ledger.iter_mut().find(|e| e.0 == page_va) {
                Some(e) => {
                    e.2 |= seg.writable;
                    e.3 |= !seg.executable;
                    (e.1, e.2, e.3)
                }
                None => {
                    let Some(p) = mp.alloc_zero_frame() else {
                        load_err = Some(LoadError::OutOfMemory);
                        return;
                    };
                    ledger.push((page_va, p, seg.writable, !seg.executable));
                    new_frame = true;
                    (p, seg.writable, !seg.executable)
                }
            };
            // 文件内容按页内真实偏移拷贝（段可以从页中间开始）。
            if k * PAGE < seg.filesz {
                let file_off = (seg.offset + k * PAGE) as usize;
                let n = core::cmp::min((PAGE - in_page) as usize, (seg.filesz - k * PAGE) as usize);
                if file_off + n > blob.len() {
                    load_err = Some(LoadError::BadBlob);
                    if new_frame {
                        // 帧已分配但尚未落账——不回收就成了黑洞。
                        mp.free_frame(phys & P_ADDR_MASK);
                    }
                    return;
                }
                mp.write_frame_bytes(phys & P_ADDR_MASK, in_page, &blob[file_off..file_off + n]);
            }
            if !mp.map_user_frame(page_va, phys & P_ADDR_MASK, w, nx) {
                load_err = Some(LoadError::MapFailed);
                if new_frame {
                    mp.free_frame(phys & P_ADDR_MASK);
                }
                return;
            }
            mp.flush(page_va);
            // 账本每页只记一次（同页多段共享帧——重复记账会让退出回收
            // 双重摘叶、数量对不上）。
            if new_frame {
                pages.push((page_va, phys & P_ADDR_MASK));
            }
        }
    });

    if let Some(e) = load_err {
        release_pages(&pages, mp);
        return Err(e);
    }

    // 用户栈：零页（W|NX），从栈顶向下。
    for k in 0..stack_pages {
        let va = stack_top - (k + 1) * PAGE;
        let Some(p) = mp.alloc_zero_frame() else {
            release_pages(&pages, mp);
            return Err(LoadError::OutOfMemory);
        };
        if !mp.map_user_frame(va, p, true, true) {
            mp.free_frame(p);
            release_pages(&pages, mp);
            return Err(LoadError::MapFailed);
        }
        mp.flush(va);
        pages.push((va, p));
    }

    Ok(LoadedImage {
        entry: src.entry(),
        stack_top,
        frames_used: pages.len() as u64,
        pages,
    })
}

/// 退出回收：摘叶、核对原帧、归还帧。返回成功释放的页数；摘叶结果与
/// 账本不符（竞态/重复回收）时如实跳过并 flush 防脏 TLB——绝不假装成功。
pub fn release_pages(pages: &[(u64, u64)], mp: &mut dyn UserMapper) -> usize {
    let mut n = 0usize;
    for &(va, phys) in pages {
        match mp.unmap_user(va) {
            Some(p) if p == phys & P_ADDR_MASK => {
                mp.free_frame(p);
                n += 1;
            }
            _ => mp.flush(va),
        }
    }
    n
}

// ---------------------------------------------------------------------------
// ELF 适配器
// ---------------------------------------------------------------------------

use super::elf::ElfImage;
use crate::mem::paging;

/// ELF → [`ImageFormat`]。W^X 沿用 `LoadSegment::page_flags` 的口径：
/// 可写段强制 NX，因此 `executable=false`。
pub struct ElfSource<'a> {
    pub img: &'a ElfImage,
    pub blob: &'a [u8],
}

impl ImageFormat for ElfSource<'_> {
    fn entry(&self) -> u64 {
        self.img.entry
    }
    fn blob(&self) -> &[u8] {
        self.blob
    }
    fn for_each_segment(&self, f: &mut dyn FnMut(SegmentView)) {
        for seg in self.img.segments() {
            let flags = seg.page_flags();
            f(SegmentView {
                vaddr: seg.vaddr,
                offset: seg.offset,
                filesz: seg.filesz,
                memsz: seg.memsz,
                writable: flags & paging::P_WRITE != 0,
                executable: flags & paging::P_NX == 0,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// 目标态：KernelMapper（真页表 + PMM + HHDM）
// ---------------------------------------------------------------------------

/// 目标态 UserMapper：包一层 [`crate::mem::pfh::PageTableOps`]，
/// 帧分配/归还直达 PMM，帧写入走 HHDM。泛型而非 dyn——零额外间接层。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub struct KernelMapper<T: crate::mem::pfh::PageTableOps> {
    inner: T,
}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
impl<T: crate::mem::pfh::PageTableOps> KernelMapper<T> {
    pub fn new(inner: T) -> Self {
        KernelMapper { inner }
    }
}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
impl<T: crate::mem::pfh::PageTableOps> UserMapper for KernelMapper<T> {
    fn alloc_zero_frame(&mut self) -> Option<u64> {
        self.inner.alloc_zero_frame()
    }
    fn map_user_frame(&mut self, va: u64, phys: u64, w: bool, nx: bool) -> bool {
        self.inner.map_user_frame(va, phys, w, nx)
    }
    fn write_frame_bytes(&mut self, phys: u64, off: u64, data: &[u8]) {
        let hhdm = crate::limine::hhdm_offset().unwrap_or(0);
        // SAFETY: 帧由引擎从 PMM 独占分配，HHDM 全覆盖；区间不越页
        // （调用方保证 off + data.len() ≤ 4096）。
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                (phys + off + hhdm) as *mut u8,
                data.len(),
            );
        }
    }
    fn flush(&mut self, va: u64) {
        self.inner.flush(va);
    }
    fn unmap_user(&mut self, va: u64) -> Option<u64> {
        let p = self.inner.unmap_user(va);
        if p.is_some() {
            self.inner.flush(va);
        }
        p
    }
    fn free_frame(&mut self, phys: u64) {
        if !crate::mem::pmm::free_order(phys, 0) {
            crate::kwarn!("loader: pmm rejected frame {:#x} — leaked one frame", phys);
        }
    }
}

// ---------------------------------------------------------------------------
// 宿主测试：同页合并 / BSS / ELF≡格式中立镜像 / 超大镜像 / 回收 / 回滚
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 宿主假内存模型：帧存在本进程 Vec 里，物理地址只是帧号的花体写法。
    /// 无全局状态——每个测试一套独立内存，天然并行安全。
    const FRAME_BASE: u64 = 0x1000_0000;

    struct HostMapper {
        map: Vec<(u64, u64, bool, bool)>,
        frames: Vec<[u8; PAGE as usize]>,
        freed: Vec<u64>,
    }
    impl HostMapper {
        fn new() -> Self {
            HostMapper {
                map: Vec::new(),
                frames: Vec::new(),
                freed: Vec::new(),
            }
        }
        fn frame_idx(&self, phys: u64) -> usize {
            ((phys - FRAME_BASE) / PAGE) as usize
        }
        fn read_va(&self, va: u64, n: usize) -> Vec<u8> {
            let (_, phys, _, _) = *self.map.iter().find(|e| e.0 == va & !0xFFF).unwrap();
            let in_page = (va & 0xFFF) as usize;
            let f = &self.frames[self.frame_idx(phys)];
            f[in_page..in_page + n].to_vec()
        }
        fn mapped(&self) -> usize {
            self.map.len()
        }
        fn flags_of(&self, va: u64) -> Option<(bool, bool)> {
            self.map
                .iter()
                .find(|e| e.0 == va & !0xFFF)
                .map(|e| (e.2, e.3))
        }
    }
    impl UserMapper for HostMapper {
        fn alloc_zero_frame(&mut self) -> Option<u64> {
            let phys = FRAME_BASE + (self.frames.len() as u64) * PAGE;
            self.frames.push([0u8; PAGE as usize]);
            Some(phys)
        }
        fn map_user_frame(&mut self, va: u64, phys: u64, w: bool, nx: bool) -> bool {
            if va >> 47 != 0 {
                return false;
            }
            match self.map.iter_mut().find(|e| e.0 == va) {
                Some(e) => *e = (va, phys, w, nx),
                None => self.map.push((va, phys, w, nx)),
            }
            true
        }
        fn write_frame_bytes(&mut self, phys: u64, off: u64, data: &[u8]) {
            let idx = self.frame_idx(phys);
            let f = &mut self.frames[idx];
            f[off as usize..off as usize + data.len()].copy_from_slice(data);
        }
        fn flush(&mut self, _va: u64) {}
        fn unmap_user(&mut self, va: u64) -> Option<u64> {
            let i = self.map.iter().position(|e| e.0 == va)?;
            let (_, phys, _, _) = self.map.remove(i);
            Some(phys)
        }
        fn free_frame(&mut self, phys: u64) {
            self.freed.push(phys);
        }
    }

    /// 格式中立"PE 风格"镜像：直接给 SegmentView 表——引擎不问出处。
    struct PeLike {
        entry: u64,
        blob: Vec<u8>,
        segs: Vec<SegmentView>,
    }
    impl ImageFormat for PeLike {
        fn entry(&self) -> u64 {
            self.entry
        }
        fn blob(&self) -> &[u8] {
            &self.blob
        }
        fn for_each_segment(&self, f: &mut dyn FnMut(SegmentView)) {
            for &s in &self.segs {
                f(s);
            }
        }
    }

    const STACK_TOP: u64 = 0x7FFF_F000;
    const STACK_PAGES: u64 = 2;

    #[test]
    fn same_page_segments_share_frame_with_flag_union() {
        // 复刻 hello.elf 的同页布局：.rodata@0x401000 与 .data@0x401088 同页。
        let mut ro = vec![0xA5u8; 0x81];
        ro[0] = b'h';
        ro[1] = b'i';
        let mut data = vec![0x5Bu8; 8];
        data[0] = 0x42;
        let mut blob = vec![0u8; 0x200];
        blob[0x10..0x10 + 0x81].copy_from_slice(&ro);
        blob[0xA0..0xA8].copy_from_slice(&data);
        let img = PeLike {
            entry: 0x400030,
            blob,
            segs: vec![
                SegmentView {
                    vaddr: 0x400000,
                    offset: 0x0,
                    filesz: 0x30,
                    memsz: 0x30,
                    writable: false,
                    executable: true,
                },
                SegmentView {
                    vaddr: 0x401000,
                    offset: 0x10,
                    filesz: 0x81,
                    memsz: 0x81,
                    writable: false,
                    executable: true,
                },
                SegmentView {
                    vaddr: 0x401088,
                    offset: 0xA0,
                    filesz: 8,
                    memsz: 0x108, // 段内 BSS
                    writable: true,
                    executable: false,
                },
            ],
        };
        let mut mp = HostMapper::new();
        let loaded = load_into(&img, &mut mp, STACK_PAGES, STACK_TOP).expect("load ok");

        // 0x400000 与 0x401000 两页 + 2 栈页 = 4 页；0x401000 只能有一帧。
        assert_eq!(loaded.frames_used, 4);
        assert_eq!(loaded.pages.len(), 4);
        assert_eq!(mp.mapped(), 4);

        // 同页两段内容各自精确落位（页内偏移拷贝）。
        let ro_page = mp.read_va(0x401000, 0x81);
        assert_eq!(&ro_page[..2], b"hi", "R 段内容必须在 0x401000 帧内");
        let data_at = mp.read_va(0x401088, 8);
        assert_eq!(data_at[0], 0x42, "W 段内容必须落在同一帧的 0x88 偏移");

        // 同页 BSS（0x401090..0x401108）保持零——页内零语义不被污染。
        let bss = mp.read_va(0x401090, 0x78);
        assert!(bss.iter().all(|&b| b == 0), "同页 BSS 必须为零");

        // flags 并集：R|X 段与 W 段同页 → 该页 W=1、NX=1（任一要 W 即 W）。
        assert_eq!(mp.flags_of(0x401000), Some((true, true)));

        // 纯 RX 页保持 W=0，且可执行 → nx=0（nx 是"不可执行"位）。
        assert_eq!(mp.flags_of(0x400000), Some((false, false)));
        assert_eq!(loaded.entry, 0x400030);
        assert_eq!(loaded.stack_top, STACK_TOP);
    }

    #[test]
    fn bss_beyond_filesz_is_zero() {
        let mut blob = vec![0xEEu8; 0x100];
        for b in blob.iter_mut().take(0x10) {
            *b = 0x77;
        }
        let img = PeLike {
            entry: 0x400000,
            blob,
            segs: vec![SegmentView {
                vaddr: 0x400000,
                offset: 0,
                filesz: 0x10,
                memsz: 0x1810, // 跨两页：filesz 之后整页 BSS
                writable: true,
                executable: false,
            }],
        };
        let mut mp = HostMapper::new();
        let loaded = load_into(&img, &mut mp, 1, STACK_TOP).unwrap();
        // filesz 内 = 0x77，filesz 后（页内 BSS）= 0，第二页全零。
        let p0 = mp.read_va(0x400000, PAGE as usize);
        assert!(p0[..0x10].iter().all(|&b| b == 0x77));
        assert!(p0[0x10..].iter().all(|&b| b == 0));
        let p1 = mp.read_va(0x401000, PAGE as usize);
        assert!(p1.iter().all(|&b| b == 0));
        assert_eq!(loaded.frames_used, 3); // 2 段页 + 1 栈页
    }

    #[test]
    fn elf_adapter_equals_format_neutral_mirror() {
        // 用真实 hello.elf 走 ElfSource，再以同段表的 PeLike 镜像走引擎——
        // 两条路径的布局必须逐页逐字节一致：引擎对格式零知识的结构性证明
        // （未来的 PE 适配器走的正是 PeLike 这条路）。
        static HELLO: &[u8] = include_bytes!("hello.elf");
        let parsed = crate::proc::elf::parse(HELLO).expect("hello.elf 必须可解析");
        let src = ElfSource {
            img: &parsed,
            blob: HELLO,
        };
        let mut segs = Vec::new();
        src.for_each_segment(&mut |s| segs.push(s));
        let mirror = PeLike {
            entry: parsed.entry,
            blob: HELLO.to_vec(),
            segs,
        };
        let mut mp1 = HostMapper::new();
        let mut mp2 = HostMapper::new();
        let a = load_into(&src, &mut mp1, 4, STACK_TOP).unwrap();
        let b = load_into(&mirror, &mut mp2, 4, STACK_TOP).unwrap();
        assert_eq!(a.entry, b.entry);
        assert_eq!(a.pages, b.pages, "页账本必须逐页一致");
        assert_eq!(a.frames_used, b.frames_used);
        for (va, phys) in &a.pages {
            let x = mp1.read_va(*va, PAGE as usize);
            let (_, phys2, _, _) = *mp2.map.iter().find(|e| e.0 == *va).unwrap();
            let f = &mp2.frames[mp2.frame_idx(phys2)];
            assert_eq!(&x[..], &f[..], "页 {:#x} 内容必须一致", va);
            assert_eq!(*phys & P_ADDR_MASK, phys2);
        }
    }

    #[test]
    fn large_image_beyond_fixed_cap_loads() {
        // 旧引擎是 32 项定容数组——48 页镜像必须照常装载（Vec 无隐形上限）。
        let blob = vec![0x11u8; 48 * 4096];
        let img = PeLike {
            entry: 0x400000,
            blob,
            segs: vec![SegmentView {
                vaddr: 0x400000,
                offset: 0,
                filesz: 48 * 4096,
                memsz: 48 * 4096,
                writable: false,
                executable: true,
            }],
        };
        let mut mp = HostMapper::new();
        let loaded = load_into(&img, &mut mp, STACK_PAGES, STACK_TOP).unwrap();
        assert_eq!(loaded.frames_used, 48 + STACK_PAGES);
        assert_eq!(mp.mapped(), (48 + STACK_PAGES) as usize);
    }

    #[test]
    fn release_pages_unmaps_and_frees_exactly() {
        let blob = vec![0x22u8; 3 * 4096];
        let img = PeLike {
            entry: 0x400000,
            blob,
            segs: vec![SegmentView {
                vaddr: 0x400000,
                offset: 0,
                filesz: 3 * 4096,
                memsz: 3 * 4096,
                writable: false,
                executable: true,
            }],
        };
        let mut mp = HostMapper::new();
        let loaded = load_into(&img, &mut mp, STACK_PAGES, STACK_TOP).unwrap();
        let n = loaded.pages.len();
        let freed = release_pages(&loaded.pages, &mut mp);
        assert_eq!(freed, n, "账本内每页都必须可摘除归还");
        assert_eq!(mp.mapped(), 0, "映射必须清空");
        assert_eq!(mp.freed.len(), n);
        // 重复回收：摘不到叶 → 返回 0，不假装成功。
        assert_eq!(release_pages(&loaded.pages, &mut mp), 0);
    }

    #[test]
    fn bad_blob_rolls_back_all_mappings() {
        let blob = vec![0x33u8; 0x40]; // 段声明 0x200 内容 → 越界
        let img = PeLike {
            entry: 0x400000,
            blob,
            segs: vec![
                SegmentView {
                    vaddr: 0x400000,
                    offset: 0,
                    filesz: 0x30,
                    memsz: 0x30,
                    writable: false,
                    executable: true,
                },
                SegmentView {
                    vaddr: 0x401000,
                    offset: 0x100, // 越过 0x40 blob → BadBlob
                    filesz: 0x200,
                    memsz: 0x200,
                    writable: false,
                    executable: true,
                },
            ],
        };
        let mut mp = HostMapper::new();
        let err = load_into(&img, &mut mp, STACK_PAGES, STACK_TOP).unwrap_err();
        assert_eq!(err, LoadError::BadBlob);
        assert_eq!(mp.mapped(), 0, "失败路径必须回滚全部映射");
        // 归还 = 1（seg1 已映射页回滚）+ 1（seg2 新帧分配后立即发现
        // BadBlob、未落账即回收）= 2，一帧不漏。
        assert_eq!(mp.freed.len(), 2);
        // 回滚后栈从未映射（失败在段阶段）。
        assert!(mp.flags_of(STACK_TOP - PAGE).is_none());
    }
}
