//! UNX-F11306 TLS 块拷贝与 init 镜像（0xe910 段）
//!
//! 每线程的 TLS 块由装载期镜像成型：前段 tdata 是已初始化字节，必须原样
//! 拷贝；后段 tbss 是零初始化字节，必须清零；两段之和就是块尺寸 memsz。
//! 块之外的竞技场字节保持脏值（0xa5 毒化），既证明写入范围精确，也让
//! “多写一字节”这类越界立刻暴露——若拷贝循环多写一位，毒值被覆盖，
//! 干净性判定即失败。
//!
//! 镜像不可变，块可变：同一镜像可以为任意多个线程成型出彼此独立的块。
//! 线程 A 改自己的块，线程 B 与镜像本身都不受影响；重新成型得到逐字节
//! 相同的块。这条“镜像-块”分离是 TLS 正确性的地基，也是本模型的首要
//! 不变量：镜像只读、块私有、重建幂等。
//!
//! 成型的三道闸门：尺寸为零或超过单块上限拒绝、对齐必须是 2 的幂、
//! 起点必须落在对齐网格上。闸门顺序固定，拒绝路径不写任何字节——账本
//! 以“线程游标不变、块计数不加”作为零副作用的证据。
//!
//! 账本十四格：六格成功（四格建块、一格异线程重建、一格幂等重放），
//! 八格拒绝（三格尺寸、两格对齐、一格起点错位、一格模块超量、一格竞技场
//! 耗尽）。成功样本逐格记录 tdata 长度、memsz 与干净性，拒绝样本记录
//! 触发闸门的错误码。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe910;

/// 编号非法。
pub const TI_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const TI_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const TI_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const TI_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const TI_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const TI_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const TI_EBAL: u32 = SEG_BASE + 0x07;
/// 尺寸非法。
pub const TI_ESIZE: u32 = SEG_BASE + 0x08;
/// 对齐非法。
pub const TI_EALIGN: u32 = SEG_BASE + 0x09;
/// 模块超量。
pub const TI_EFULL: u32 = SEG_BASE + 0x0a;
/// 竞技场满。
pub const TI_ESPACE: u32 = SEG_BASE + 0x0b;
/// 毒值残留异常。
pub const TI_EPOISON: u32 = SEG_BASE + 0x0c;
/// 票号不一致。
pub const TI_ESEAL: u32 = SEG_BASE + 0x0d;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 单块容量。
pub const BLOCK_CAP: usize = 64;
/// 单线程模块数上限。
pub const MODS: usize = 4;
/// 竞技场容量。
pub const ARENA_CAP: u32 = 120;
/// 线程竞技场起点。
pub const ARENA_START: u32 = 64;
/// 毒化字节。
pub const POISON: u8 = 0xa5;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 2 的幂判定（usize）。
pub fn is_pow2(v: usize) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

/// 上取整对齐。
pub fn align_up(v: u32, a: usize) -> Result<u32, u32> {
    if !is_pow2(a) {
        return Err(TI_EALIGN);
    }
    let mask = a as u32 - 1;
    match v.checked_add(mask) {
        Some(x) => Ok(x & !mask),
        None => Err(TI_ESPACE),
    }
}

/// 装载期只读镜像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TlsImage {
    data: [u8; BLOCK_CAP],
    pub tdata_len: usize,
    pub tbss_len: usize,
    pub align: usize,
}

impl TlsImage {
    /// 构造镜像：tdata 原样保存，tbss 只记录长度。
    pub fn make(tdata: &[u8], tbss_len: usize, align: usize) -> Result<TlsImage, u32> {
        if tdata.len() > BLOCK_CAP {
            return Err(TI_ESIZE);
        }
        let memsz = match tdata.len().checked_add(tbss_len) {
            Some(v) => v,
            None => return Err(TI_ESIZE),
        };
        if memsz == 0 || memsz > BLOCK_CAP {
            return Err(TI_ESIZE);
        }
        if !is_pow2(align) {
            return Err(TI_EALIGN);
        }
        let mut data = [0u8; BLOCK_CAP];
        let mut i = 0;
        while i < tdata.len() {
            match (data.get_mut(i), tdata.get(i)) {
                (Some(d), Some(s)) => *d = *s,
                _ => return Err(TI_ESIZE),
            }
            i += 1;
        }
        Ok(TlsImage {
            data,
            tdata_len: tdata.len(),
            tbss_len,
            align,
        })
    }

    /// 块尺寸。
    pub fn memsz(&self) -> usize {
        self.tdata_len + self.tbss_len
    }

    /// 取初始字节。
    pub fn init(&self, i: usize) -> Option<u8> {
        if i >= self.memsz() {
            return None;
        }
        if i < self.tdata_len {
            self.data.get(i).copied()
        } else {
            Some(0)
        }
    }
}

/// 一个已成型块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    bytes: [u8; BLOCK_CAP],
    memsz: usize,
    tdata: usize,
    align: usize,
    addr: u32,
}

impl Block {
    /// 空白块。
    pub const fn empty() -> Block {
        Block {
            bytes: [POISON; BLOCK_CAP],
            memsz: 0,
            tdata: 0,
            align: 1,
            addr: 0,
        }
    }

    /// 由镜像成型：先拷 tdata，再清零 tbss，余下保持毒值。
    pub fn materialize(img: &TlsImage, start: u32) -> Result<Block, u32> {
        let memsz = img.memsz();
        if memsz == 0 {
            return Err(TI_ESIZE);
        }
        if (start as usize) % img.align != 0 {
            return Err(TI_EALIGN);
        }
        let mut bytes = [POISON; BLOCK_CAP];
        let mut i = 0;
        while i < img.tdata_len {
            match (bytes.get_mut(i), img.data.get(i)) {
                (Some(d), Some(s)) => *d = *s,
                _ => return Err(TI_ESIZE),
            }
            i += 1;
        }
        while i < memsz {
            match bytes.get_mut(i) {
                Some(d) => *d = 0,
                None => return Err(TI_ESIZE),
            }
            i += 1;
        }
        Ok(Block {
            bytes,
            memsz,
            tdata: img.tdata_len,
            align: img.align,
            addr: start,
        })
    }

    /// 取字节。
    pub fn byte(&self, i: usize) -> Option<u8> {
        if i >= self.memsz {
            return None;
        }
        self.bytes.get(i).copied()
    }

    /// 取块外字节（应为毒值）。
    pub fn raw(&self, i: usize) -> Option<u8> {
        self.bytes.get(i).copied()
    }

    /// 块基址。
    pub fn addr(&self) -> u32 {
        self.addr
    }

    /// 块内 tdata 长度。
    pub fn tdata(&self) -> usize {
        self.tdata
    }

    /// 块尺寸。
    pub fn memsz(&self) -> usize {
        self.memsz
    }

    /// 与镜像逐字节一致：tdata 吻合、tbss 为零、块外仍是毒值。
    pub fn clean(&self, img: &TlsImage) -> bool {
        if self.memsz != img.memsz() || self.tdata != img.tdata_len {
            return false;
        }
        let mut i = 0;
        while i < self.memsz {
            match (self.bytes.get(i), img.init(i)) {
                (Some(b), Some(w)) => {
                    if *b != w {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        while i < BLOCK_CAP {
            match self.bytes.get(i) {
                Some(b) => {
                    if *b != POISON {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 毒值是否完好（块外未被写）。
    pub fn poison_intact(&self) -> bool {
        let mut i = self.memsz;
        while i < BLOCK_CAP {
            match self.bytes.get(i) {
                Some(b) => {
                    if *b != POISON {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 一个线程的私有块集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thread {
    pub id: u16,
    blocks: [Block; MODS],
    n: usize,
    cursor: u32,
}

impl Thread {
    /// 空白线程。
    pub const fn empty() -> Thread {
        Thread {
            id: 0,
            blocks: [Block::empty(); MODS],
            n: 0,
            cursor: ARENA_START,
        }
    }

    /// 按镜像顺序成型全部块。
    pub fn spawn(imgs: &[TlsImage], id: u16) -> Result<Thread, u32> {
        if imgs.len() > MODS {
            return Err(TI_EFULL);
        }
        let mut th = Thread::empty();
        th.id = id;
        let mut i = 0;
        while i < imgs.len() {
            let img = match imgs.get(i) {
                Some(v) => *v,
                None => return Err(TI_ENOBS),
            };
            if th.n != i {
                return Err(TI_EORDER);
            }
            let start = match align_up(th.cursor, img.align) {
                Ok(v) => v,
                Err(e) => return Err(e),
            };
            let end = match start.checked_add(img.memsz() as u32) {
                Some(v) => v,
                None => return Err(TI_ESPACE),
            };
            if end > ARENA_CAP {
                return Err(TI_ESPACE);
            }
            let b = match Block::materialize(&img, start) {
                Ok(v) => v,
                Err(e) => return Err(e),
            };
            match th.blocks.get_mut(i) {
                Some(s) => *s = b,
                None => return Err(TI_EFULL),
            }
            th.n += 1;
            th.cursor = end;
            i += 1;
        }
        Ok(th)
    }

    /// 块数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 取块。
    pub fn block(&self, i: usize) -> Option<Block> {
        if i >= self.n {
            return None;
        }
        self.blocks.get(i).copied()
    }

    /// 游标。
    pub fn cursor(&self) -> u32 {
        self.cursor
    }

    /// 改块内一个字节。
    pub fn poke(&mut self, i: usize, off: usize, v: u8) -> Result<(), u32> {
        if i >= self.n {
            return Err(TI_ENOBS);
        }
        let b = match self.blocks.get_mut(i) {
            Some(v) => v,
            None => return Err(TI_ENOBS),
        };
        if off >= b.memsz {
            return Err(TI_ESIZE);
        }
        match b.bytes.get_mut(off) {
            Some(s) => *s = v,
            None => return Err(TI_ENOBS),
        }
        Ok(())
    }

    /// 读块内一个字节。
    pub fn peek(&self, i: usize, off: usize) -> Option<u8> {
        if i >= self.n {
            return None;
        }
        let b = self.blocks.get(i)?;
        if off >= b.memsz {
            return None;
        }
        b.bytes.get(off).copied()
    }

    /// 全部块与镜像一致。
    pub fn clean(&self, imgs: &[TlsImage]) -> bool {
        if self.n != imgs.len() {
            return false;
        }
        let mut i = 0;
        while i < self.n {
            match (self.blocks.get(i), imgs.get(i)) {
                (Some(b), Some(img)) => {
                    if !b.clean(img) {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub world: u8,
    pub ok: bool,
    pub code: u32,
    pub addr: u32,
    pub tdata: u8,
    pub memsz: u8,
    pub clean: bool,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            world: 0,
            ok: false,
            code: 0,
            addr: 0,
            tdata: 0,
            memsz: 0,
            clean: false,
        }
    }
}

/// FNV 折入。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    seal: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格。
    pub fn register(
        &mut self,
        slot: u8,
        world: u8,
        r: Result<(u32, bool), u32>,
        tdata: usize,
        memsz: usize,
    ) -> Result<(), u32> {
        if self.frozen {
            return Err(TI_EFROZEN);
        }
        if (slot as usize) != self.len {
            return Err(TI_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(TI_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == slot {
                        return Err(TI_EDUP);
                    }
                }
                None => return Err(TI_ENOBS),
            }
            i += 1;
        }
        let row = match r {
            Ok((addr, clean)) => {
                if addr < ARENA_START || addr >= ARENA_CAP {
                    return Err(TI_EMISMATCH);
                }
                if !clean {
                    return Err(TI_EPOISON);
                }
                Sample {
                    slot,
                    world,
                    ok: true,
                    code: V_OK,
                    addr,
                    tdata: tdata as u8,
                    memsz: memsz as u8,
                    clean,
                }
            }
            Err(c) => {
                if c == V_OK {
                    return Err(TI_EMISMATCH);
                }
                Sample {
                    slot,
                    world,
                    ok: false,
                    code: c,
                    addr: 0,
                    tdata: tdata as u8,
                    memsz: memsz as u8,
                    clean: false,
                }
            }
        };
        match self.rows.get_mut(self.len) {
            Some(s) => *s = row,
            None => return Err(TI_EID),
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 取地址。
    pub fn addr(&self, i: usize) -> Option<u32> {
        match self.sample(i) {
            Some(s) => {
                if s.ok {
                    Some(s.addr)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// 成功计数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 拒绝计数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(TI_EBAL);
        }
        if self.ok_count() != 6 || self.err_count() != 8 {
            return Err(TI_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(TI_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK || !s.clean || s.addr == 0 {
                            return Err(TI_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.clean || s.addr != 0 {
                        return Err(TI_EMISMATCH);
                    }
                }
                None => return Err(TI_ENOBS),
            }
            i += 1;
        }
        let addrs = [64u32, 80, 88, 104];
        let mut k = 0;
        while k < addrs.len() {
            match addrs.get(k) {
                Some(a) => {
                    if self.addr(k) != Some(*a) {
                        return Err(TI_EMISMATCH);
                    }
                }
                None => return Err(TI_EBAL),
            }
            k += 1;
        }
        let shapes = [(4u8, 8u8), (0, 8), (12, 16), (1, 1)];
        let mut q = 0;
        while q < shapes.len() {
            match (self.sample(q), shapes.get(q)) {
                (Some(s), Some(w)) => {
                    if s.tdata != w.0 || s.memsz != w.1 {
                        return Err(TI_EMISMATCH);
                    }
                }
                _ => return Err(TI_ENOBS),
            }
            q += 1;
        }
        if self.addr(12) != self.addr(0) || self.addr(13) != self.addr(0) {
            return Err(TI_EMISMATCH);
        }
        let codes = [
            TI_ESPACE, TI_ESIZE, TI_ESIZE, TI_ESIZE, TI_EALIGN, TI_EALIGN, TI_EALIGN, TI_EFULL,
        ];
        let mut c = 0;
        let mut j = 0;
        while j < self.len {
            if let Some(s) = self.rows.get(j) {
                if !s.ok {
                    match codes.get(c) {
                        Some(w) => {
                            if s.code != *w {
                                return Err(TI_EMISMATCH);
                            }
                        }
                        None => return Err(TI_EBAL),
                    }
                    c += 1;
                }
            }
            j += 1;
        }
        if c != codes.len() {
            return Err(TI_EBAL);
        }
        Ok(())
    }

    /// 熔断票号。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.world as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.addr as u64);
                h = fold(h, s.tdata as u64);
                h = fold(h, s.memsz as u64);
                h = fold(h, s.clean as u64);
            }
            i += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(TI_EFROZEN);
        }
        let s = self.seal();
        self.seal = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(TI_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(TI_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.seal
    }

    /// 二次冻结次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 标准四镜像：A 四字节 tdata + 四字节 tbss；B 纯 tbss；C 十二字节 tdata；D 单字节。
pub fn standard_images() -> [TlsImage; 4] {
    let a = match TlsImage::make(&[1, 2, 3, 4], 4, 8) {
        Ok(v) => v,
        Err(_) => unreachable_image(),
    };
    let b = match TlsImage::make(&[], 8, 16) {
        Ok(v) => v,
        Err(_) => unreachable_image(),
    };
    let c = match TlsImage::make(&[0xde, 0xad, 0xbe, 0xef, 1, 2, 3, 4, 5, 6, 7, 8], 4, 8) {
        Ok(v) => v,
        Err(_) => unreachable_image(),
    };
    let d = match TlsImage::make(&[7], 0, 1) {
        Ok(v) => v,
        Err(_) => unreachable_image(),
    };
    [a, b, c, d]
}

/// 兜底镜像（标准构造不可能失败）。
fn unreachable_image() -> TlsImage {
    TlsImage {
        data: [0u8; BLOCK_CAP],
        tdata_len: 1,
        tbss_len: 0,
        align: 1,
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let imgs = standard_images();
    let a = match imgs.get(0) {
        Some(v) => *v,
        None => return l,
    };
    let b = match imgs.get(1) {
        Some(v) => *v,
        None => return l,
    };
    let c = match imgs.get(2) {
        Some(v) => *v,
        None => return l,
    };
    let d = match imgs.get(3) {
        Some(v) => *v,
        None => return l,
    };

    let main = match Thread::spawn(&imgs, 1) {
        Ok(v) => v,
        Err(_) => Thread::empty(),
    };
    let mut m = 0usize;
    while m < main.len() {
        let (addr, clean, td, ms) = match main.block(m) {
            Some(blk) => {
                let img = match imgs.get(m) {
                    Some(v) => *v,
                    None => break,
                };
                (blk.addr(), blk.clean(&img), blk.tdata(), blk.memsz())
            }
            None => break,
        };
        let _ = l.register(m as u8, 0, Ok((addr, clean)), td, ms);
        m += 1;
    }

    let e = match TlsImage::make(&[9], 63, 64) {
        Ok(v) => v,
        Err(_) => unreachable_image(),
    };
    let space = Thread::spawn(&[e], 2);
    let space_r = match space {
        Ok(th) => Ok((th.block(0).map(|x| x.addr()).unwrap_or(0), true)),
        Err(code) => Err(code),
    };
    let _ = l.register(4, 1, space_r, e.tdata_len, e.memsz());

    let over = TlsImage::make(&[0u8; 65], 0, 8);
    let over_r = match over {
        Ok(_) => Ok((0u32, true)),
        Err(code) => Err(code),
    };
    let over_t = match over {
        Ok(img) => (img.tdata_len, img.memsz()),
        Err(_) => (65usize, 65usize),
    };
    let _ = l.register(5, 2, over_r, over_t.0, over_t.1);

    let mixed = TlsImage::make(&[0u8; 60], 8, 8);
    let mixed_r = match mixed {
        Ok(_) => Ok((0u32, true)),
        Err(code) => Err(code),
    };
    let mixed_t = match mixed {
        Ok(img) => (img.tdata_len, img.memsz()),
        Err(_) => (60usize, 68usize),
    };
    let _ = l.register(6, 2, mixed_r, mixed_t.0, mixed_t.1);

    let zero = TlsImage::make(&[], 0, 8);
    let zero_r = match zero {
        Ok(_) => Ok((0u32, true)),
        Err(code) => Err(code),
    };
    let zero_t = match zero {
        Ok(img) => (img.tdata_len, img.memsz()),
        Err(_) => (0usize, 0usize),
    };
    let _ = l.register(7, 2, zero_r, zero_t.0, zero_t.1);

    let odd = TlsImage::make(&[1, 2], 2, 3);
    let odd_r = match odd {
        Ok(_) => Ok((0u32, true)),
        Err(code) => Err(code),
    };
    let odd_t = match odd {
        Ok(img) => (img.tdata_len, img.memsz()),
        Err(_) => (2usize, 4usize),
    };
    let _ = l.register(8, 2, odd_r, odd_t.0, odd_t.1);

    let bad_align = TlsImage::make(&[1, 2], 2, 0);
    let bad_align_r = match bad_align {
        Ok(_) => Ok((0u32, true)),
        Err(code) => Err(code),
    };
    let bad_align_t = match bad_align {
        Ok(img) => (img.tdata_len, img.memsz()),
        Err(_) => (2usize, 4usize),
    };
    let _ = l.register(9, 2, bad_align_r, bad_align_t.0, bad_align_t.1);

    let misstart = Block::materialize(&a, 65);
    let misstart_r = match misstart {
        Ok(blk) => Ok((blk.addr(), blk.clean(&a))),
        Err(code) => Err(code),
    };
    let _ = l.register(10, 3, misstart_r, a.tdata_len, a.memsz());

    let four = Thread::spawn(&[a, b, c, d, a], 3);
    let four_r = match four {
        Ok(th) => Ok((th.block(0).map(|x| x.addr()).unwrap_or(0), true)),
        Err(code) => Err(code),
    };
    let _ = l.register(11, 3, four_r, a.tdata_len, a.memsz());

    let other = match Thread::spawn(&[a], 2) {
        Ok(v) => v,
        Err(_) => Thread::empty(),
    };
    let other_r = match other.block(0) {
        Some(blk) => Ok((blk.addr(), blk.clean(&a))),
        None => Err(TI_ENOBS),
    };
    let _ = l.register(12, 4, other_r, a.tdata_len, a.memsz());

    let again = Block::materialize(&a, 64);
    let again_r = match again {
        Ok(blk) => Ok((blk.addr(), blk.clean(&a))),
        Err(code) => Err(code),
    };
    let _ = l.register(13, 4, again_r, a.tdata_len, a.memsz());

    l
}

/// 四路注入：账本重放、镜像精确性、线程独立性、拒绝零副作用。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let imgs = standard_images();
    let a = match imgs.get(0) {
        Some(v) => *v,
        None => return out,
    };
    let blk = Block::materialize(&a, 64);
    out[1] = match blk {
        Ok(b) => {
            b.byte(0) == Some(1)
                && b.byte(3) == Some(4)
                && b.byte(4) == Some(0)
                && b.byte(7) == Some(0)
                && b.byte(8) == None
                && b.raw(8) == Some(POISON)
                && b.clean(&a)
                && b.poison_intact()
        }
        Err(_) => false,
    };

    let t1 = Thread::spawn(&imgs, 1);
    let t2 = Thread::spawn(&imgs, 2);
    out[2] = match (t1, t2) {
        (Ok(mut one), Ok(two)) => {
            let before = two.peek(0, 0);
            let poked = one.poke(0, 0, 0x55);
            poked.is_ok()
                && one.peek(0, 0) == Some(0x55)
                && two.peek(0, 0) == before
                && two.clean(&imgs)
                && one.block(0).map(|b| b.clean(&a)) == Some(false)
        }
        _ => false,
    };

    let deny = Thread::empty();
    let before = deny.cursor();
    let bad = match TlsImage::make(&[1], 0, 3) {
        Ok(img) => Thread::spawn(&[img], 1),
        Err(code) => Err(code),
    };
    let four = Thread::spawn(&[a, a, a, a, a], 1);
    out[3] = bad == Err(TI_EALIGN)
        && deny.cursor() == before
        && deny.len() == 0
        && four == Err(TI_EFULL)
        && deny.block(0).is_none();

    out
}

/// 十三项判据。
pub fn run_unx11306_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11306.tlsimage");
    let l = build_ledger();
    let imgs = standard_images();
    let a = imgs[0];
    let c = imgs[2];

    s.add(
        "TI1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.world) == Some(0),
        "static",
    );

    s.add_dyn(
        "TI2-六允八拒分布正确",
        l.ok_count() == 6 && l.err_count() == 8 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    let main = Thread::spawn(&imgs, 1);
    s.add(
        "TI3-tdata 前缀逐字节吻合",
        match main {
            Ok(th) => {
                th.peek(0, 0) == Some(1)
                    && th.peek(0, 3) == Some(4)
                    && th.peek(2, 0) == Some(0xde)
                    && th.peek(2, 11) == Some(8)
                    && th.peek(3, 0) == Some(7)
            }
            Err(_) => false,
        },
        "static",
    );

    s.add(
        "TI4-tbss 尾段清零",
        match main {
            Ok(th) => {
                th.peek(0, 4) == Some(0)
                    && th.peek(0, 7) == Some(0)
                    && th.peek(1, 0) == Some(0)
                    && th.peek(1, 7) == Some(0)
                    && th.peek(2, 12) == Some(0)
                    && th.peek(2, 15) == Some(0)
                    && th.block(0).map(|b| b.poison_intact()) == Some(true)
            }
            Err(_) => false,
        },
        "static",
    );

    s.add(
        "TI5-块起点对齐",
        l.addr(0) == Some(64)
            && l.addr(1) == Some(80)
            && l.addr(2) == Some(88)
            && l.addr(3) == Some(104)
            && main.as_ref().map(|th| th.cursor()).unwrap_or(0) == 105,
        "static",
    );

    let indep = match (Thread::spawn(&imgs, 1), Thread::spawn(&imgs, 2)) {
        (Ok(mut one), Ok(two)) => {
            let ok = one.poke(2, 12, 0x7f).is_ok();
            ok && one.peek(2, 12) == Some(0x7f)
                && two.peek(2, 12) == Some(0)
                && two.clean(&imgs)
        }
        _ => false,
    };
    s.add("TI6-每线程独立镜像", indep, "static");

    s.add(
        "TI7-重建幂等",
        l.addr(13) == l.addr(0)
            && match (Block::materialize(&a, 64), Block::materialize(&a, 64)) {
                (Ok(x), Ok(y)) => x == y && x.clean(&a),
                _ => false,
            },
        "static",
    );

    s.add(
        "TI8-尺寸闸门",
        l.sample(5).map(|x| x.code) == Some(TI_ESIZE)
            && l.sample(6).map(|x| x.code) == Some(TI_ESIZE)
            && l.sample(7).map(|x| x.code) == Some(TI_ESIZE)
            && TlsImage::make(&[0u8; 61], 4, 8) == Err(TI_ESIZE),
        "static",
    );

    s.add(
        "TI9-对齐闸门",
        l.sample(8).map(|x| x.code) == Some(TI_EALIGN)
            && l.sample(9).map(|x| x.code) == Some(TI_EALIGN)
            && l.sample(10).map(|x| x.code) == Some(TI_EALIGN)
            && Block::materialize(&a, 65) == Err(TI_EALIGN)
            && Block::materialize(&c, 88).is_ok(),
        "static",
    );

    s.add(
        "TI10-模块超量拒绝",
        l.sample(11).map(|x| x.code) == Some(TI_EFULL)
            && Thread::spawn(&[a, a, a, a, a], 0) == Err(TI_EFULL),
        "static",
    );

    s.add(
        "TI11-竞技场满拒绝",
        l.sample(4).map(|x| x.code) == Some(TI_ESPACE)
            && {
                let e = TlsImage::make(&[9], 63, 64);
                match e {
                    Ok(img) => Thread::spawn(&[img], 0) == Err(TI_ESPACE),
                    Err(_) => false,
                }
            },
        "static",
    );

    s.add(
        "TI12-拒绝零副作用",
        match Thread::spawn(&[a], 0) {
            Ok(mut th) => {
                let before = (th.cursor(), th.len());
                let deny = th.poke(1, 0, 0);
                let over = th.poke(0, 8, 0);
                deny == Err(TI_ENOBS)
                    && over == Err(TI_ESIZE)
                    && (th.cursor(), th.len()) == before
                    && th.clean(&[a])
            }
            Err(_) => false,
        },
        "static",
    );

    let mut g = build_ledger();
    let rep = g.replay();
    let root = match g.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = g.freeze() == Err(TI_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "TI13-冻结重放与四路注入",
        root != 0 && rep == Ok(root) && again && g.root() == root && g.rejects() == 1 && inj == [true; 4],
        &format!("root={:016x} inject={:?}", root, inj),
    );

    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("rows={} ok={} err={}", l.len(), l.ok_count(), l.err_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_copies_tdata_and_zeroes_tbss() {
        let img = TlsImage::make(&[0xaa, 0xbb], 6, 8).expect("image");
        let blk = Block::materialize(&img, 64).expect("block");
        assert_eq!(blk.byte(0), Some(0xaa));
        assert_eq!(blk.byte(1), Some(0xbb));
        assert_eq!(blk.byte(2), Some(0));
        assert_eq!(blk.byte(7), Some(0));
        assert_eq!(blk.byte(8), None);
        assert_eq!(blk.raw(8), Some(POISON));
    }

    #[test]
    fn blocks_are_per_thread() {
        let img = TlsImage::make(&[1, 2, 3], 1, 4).expect("image");
        let mut one = Thread::spawn(&[img], 1).expect("one");
        let two = Thread::spawn(&[img], 2).expect("two");
        assert_eq!(one.poke(0, 0, 9), Ok(()));
        assert_eq!(one.peek(0, 0), Some(9));
        assert_eq!(two.peek(0, 0), Some(1));
        assert!(two.clean(&[img]));
    }

    #[test]
    fn rebuild_is_idempotent() {
        let img = TlsImage::make(&[4, 5, 6, 7], 4, 8).expect("image");
        let a = Block::materialize(&img, 64).expect("a");
        let b = Block::materialize(&img, 64).expect("b");
        assert_eq!(a, b);
        assert!(a.clean(&img));
    }

    #[test]
    fn size_gate_rejects_oversize() {
        assert_eq!(TlsImage::make(&[0u8; 65], 0, 8), Err(TI_ESIZE));
        assert_eq!(TlsImage::make(&[0u8; 60], 8, 8), Err(TI_ESIZE));
        assert_eq!(TlsImage::make(&[], 0, 8), Err(TI_ESIZE));
    }

    #[test]
    fn align_gate_rejects_odd_align() {
        assert_eq!(TlsImage::make(&[1], 0, 3), Err(TI_EALIGN));
        assert_eq!(TlsImage::make(&[1], 0, 0), Err(TI_EALIGN));
        let img = TlsImage::make(&[1], 0, 4).expect("image");
        assert_eq!(Block::materialize(&img, 66), Err(TI_EALIGN));
        assert!(Block::materialize(&img, 68).is_ok());
    }

    #[test]
    fn module_cap_and_arena_cap() {
        let img = TlsImage::make(&[1], 1, 8).expect("image");
        assert_eq!(Thread::spawn(&[img, img, img, img, img], 0), Err(TI_EFULL));
        let big = TlsImage::make(&[1], 63, 64).expect("image");
        assert_eq!(Thread::spawn(&[big], 0), Err(TI_ESPACE));
    }

    #[test]
    fn poke_is_bounds_checked() {
        let img = TlsImage::make(&[1, 2], 2, 2).expect("image");
        let mut th = Thread::spawn(&[img], 0).expect("thread");
        assert_eq!(th.poke(0, 4, 9), Err(TI_ESIZE));
        assert_eq!(th.poke(1, 0, 9), Err(TI_ENOBS));
        assert_eq!(th.poke(0, 3, 9), Ok(()));
        assert_eq!(th.peek(0, 3), Some(9));
    }

    #[test]
    fn ledger_seals_and_replays() {
        let mut l = build_ledger();
        assert!(l.audit().is_ok());
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(TI_EFROZEN));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11306_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
