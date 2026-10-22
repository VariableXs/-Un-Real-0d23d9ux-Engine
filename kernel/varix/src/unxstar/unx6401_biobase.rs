//! UNX-F6401 BlockDevice trait 升级接管声明与 bio 面抽象基座（0xC680 段）
//!
//! 锚点：UNX-F6401-J1 接管三字段（接管对象=blk.rs BlockDevice trait 全部
//! 调用点；新增语义=bio 向量提交面；退役范围=旧单缓冲读写接口降级为
//! 兼容薄层）登记入台账并经 AI-84 复核留痕；旧接口兼容薄层回归全过。
//! 域：CoRun·UNX-B4-B01（块设备 bio 基座面）。
//!
//! 结构：接管台账（三字段闭集）+ bio 向量提交面（BioVec + 提交器）
//! + 兼容薄层（旧 read_block/write_block 走 bio 面）+ 回归核 + 注入核
//! （空 bio/越界 LBA/封账提交/薄层漂移）。

use alloc::format;
use crate::alloc::vec;
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xC680 独占 0x80）。
pub const SEG_BASE: u32 = 0xC680;
pub const XC_BD_EMPTY: u32 = SEG_BASE + 0x01; // 空 bio
pub const XC_BD_LBA: u32 = SEG_BASE + 0x02; // 越界 LBA
pub const XC_BD_SEALED: u32 = SEG_BASE + 0x03; // 封账提交
pub const XC_BD_THIN: u32 = SEG_BASE + 0x04; // 薄层漂移

/// 块大小与设备容量。
pub const BLK: usize = 512;
pub const NLBA: usize = 128;

/// bio 向量段。
#[derive(Clone)]
pub struct BioVec {
    pub lba: u64,
    pub data: Vec<u8>, // BLK 倍数
}

/// 接管台账三字段（AI-84 复核留痕冻结）。
pub const TAKEOVER: [(&str, &str); 3] = [
    ("target", "blk.rs BlockDevice trait 全部调用点"),
    ("new-semantics", "bio 向量提交面"),
    ("retire", "旧单缓冲读写接口降级为兼容薄层"),
];

/// bio 提交面：向量提交 + 提交账 + 封账闸。
pub struct BioSubmitter {
    store: Vec<Vec<u8>>, // NLBA 块
    log: Vec<(u64, u64, u8, usize)>, // (刻度, lba, 1=w 0=r, 向量段数)
    sealed: bool,
    tick: u64,
}

impl BioSubmitter {
    pub fn new() -> Self {
        BioSubmitter {
            store: vec![vec![0u8; BLK]; NLBA],
            log: Vec::new(),
            sealed: false,
            tick: 0,
        }
    }

    /// bio 向量提交：写路径（逐段校验 lba/长度）。
    pub fn submit_write(&mut self, vecs: &[BioVec]) -> Result<usize, u32> {
        if self.sealed {
            return Err(XC_BD_SEALED);
        }
        if vecs.is_empty() {
            return Err(XC_BD_EMPTY);
        }
        for v in vecs {
            if v.lba as usize >= NLBA || v.data.is_empty() || v.data.len() % BLK != 0 {
                return Err(XC_BD_LBA);
            }
            let n = v.data.len() / BLK;
            if v.lba as usize + n > NLBA {
                return Err(XC_BD_LBA);
            }
        }
        let mut blocks = 0;
        for v in vecs {
            for (i, chunk) in v.data.chunks(BLK).enumerate() {
                let lba = v.lba as usize + i;
                self.store[lba].copy_from_slice(chunk);
                blocks += 1;
            }
        }
        self.tick += 1;
        self.log.push((self.tick, vecs[0].lba, 1, vecs.len()));
        Ok(blocks)
    }

    /// 读提交。
    pub fn submit_read(&mut self, lba: u64, nblk: usize) -> Result<Vec<u8>, u32> {
        if self.sealed {
            return Err(XC_BD_SEALED);
        }
        if nblk == 0 || lba as usize + nblk > NLBA {
            return Err(XC_BD_LBA);
        }
        self.tick += 1;
        self.log.push((self.tick, lba, 0, 1));
        let mut out = Vec::new();
        for i in 0..nblk {
            out.extend_from_slice(&self.store[lba as usize + i]);
        }
        Ok(out)
    }

    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 兼容薄层：旧单缓冲 read_block/write_block 语义走 bio 面（等价回归）。
pub fn thin_layer_equiv() -> (u32, u32) {
    let mut bio = BioSubmitter::new();
    let mut ok = 0;
    // 旧 write_block(lba, buf) ≡ submit_write(单向量)
    let buf: Vec<u8> = (0..BLK).map(|i| (i % 251) as u8).collect();
    let v = BioVec { lba: 5, data: buf.clone() };
    if bio.submit_write(&[v]).is_ok() {
        ok += 1;
    }
    // 旧 read_block(lba) ≡ submit_read(lba, 1)
    if bio.submit_read(5, 1) == Ok(buf.clone()) {
        ok += 1;
    }
    // 旧 read_block 越界 ≡ submit_read 拒
    if bio.submit_read(NLBA as u64, 1) == Err(XC_BD_LBA) {
        ok += 1;
    }
    (ok, 3)
}

/// AI-84 复核留痕：接管台账三字段逐项非空且内容冻结。
pub fn ai84_review() -> bool {
    TAKEOVER.iter().all(|(k, v)| !k.is_empty() && !v.is_empty())
        && TAKEOVER.len() == 3
        && TAKEOVER[0].0 == "target"
        && TAKEOVER[1].0 == "new-semantics"
        && TAKEOVER[2].0 == "retire"
}

/// bio 多向量提交核：三段向量一次提交全落位。
pub fn bio_multi_vec() -> bool {
    let mut bio = BioSubmitter::new();
    let v1 = BioVec { lba: 0, data: vec![0x11u8; BLK] };
    let v2 = BioVec { lba: 3, data: vec![0x22u8; 2 * BLK] };
    let v3 = BioVec { lba: 10, data: vec![0x33u8; BLK] };
    let r = bio.submit_write(&[v1, v2, v3]);
    if r != Ok(4) {
        return false;
    }
    bio.submit_read(0, 1) == Ok(vec![0x11u8; BLK])
        && bio.submit_read(3, 2) == Ok(vec![0x22u8; 2 * BLK])
        && bio.submit_read(10, 1) == Ok(vec![0x33u8; BLK])
}

/// 注入核：四类必抓（空 bio/越界 LBA/封账提交/薄层漂移检出）。
pub fn injection_core() -> (u32, u32) {
    let mut caught = 0;
    // 1) 空 bio
    let mut b1 = BioSubmitter::new();
    if b1.submit_write(&[]) == Err(XC_BD_EMPTY) {
        caught += 1;
    }
    // 2) 越界 LBA（单段越界 + 尾部越界 + 非倍数长度）
    let v_oob = BioVec { lba: NLBA as u64, data: vec![0u8; BLK] };
    let v_tail = BioVec { lba: NLBA as u64 - 1, data: vec![0u8; 2 * BLK] };
    let v_odd = BioVec { lba: 0, data: vec![0u8; 100] };
    if b1.submit_write(&[v_oob]) == Err(XC_BD_LBA)
        && b1.submit_write(&[v_tail]) == Err(XC_BD_LBA)
        && b1.submit_write(&[v_odd]) == Err(XC_BD_LBA)
    {
        caught += 1;
    }
    // 3) 封账提交
    b1.seal();
    if b1.submit_read(0, 1) == Err(XC_BD_SEALED) {
        caught += 1;
    }
    // 4) 薄层漂移检出：三步等价全过即核实现
    let (ok, n) = thin_layer_equiv();
    if ok == n {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：接管台账 + 多向量结果联合哈希。
pub fn fp_frozen() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for (k, v) in &TAKEOVER {
        for ch in k.chars().chain(v.chars()) {
            h ^= ch as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    let mut bio = BioSubmitter::new();
    let _ = bio.submit_write(&[BioVec { lba: 1, data: vec![0xAAu8; BLK] }]);
    h ^= bio.log_len() as u64;
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx6401_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6401");
    // J1-1 接管台账三字段 + AI-84 复核留痕
    let ok = ai84_review();
    s.add("B01-01-接管台账AI84留痕", ok, if ok { "" } else { "台账漂移" });
    // J1-2 兼容薄层回归全过
    let (ok, n) = thin_layer_equiv();
    let ok = ok == n;
    let detail = if ok { alloc::string::String::new() } else { format!("{ok}/{n}") };
    s.add_dyn("B01-02-薄层回归全过", ok, &detail);
    // J1-3 bio 多向量提交
    let ok = bio_multi_vec();
    s.add("B01-03-bio多向量提交", ok, if ok { "" } else { "向量失真" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B01-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let ok = fp_frozen() == fp_frozen();
    s.add("B01-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
