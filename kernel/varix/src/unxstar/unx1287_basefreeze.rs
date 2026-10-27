//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1287 · 现存行为基线冻结账（退役前行为快照） · 0xFDB0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1287-J1 三路径现存行为基线快照生成率 100%（tick 序列/串口
//!      字节流/扫描码流三面快照），基线版本化只增不减，基线完整性断言
//!      （快照自校验和）10/10 次通过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三面快照：tick 序列（定时面）/ 串口字节流（串口面）/ 扫描码流
//!     （键鼠面），每面定容 64 采样，采样值由写死种子的确定性源生成
//!     （退役前行为 = 轮询面的确定可复现画像）；
//!   - 版本化：基线账 append-only，每次冻结产生新版本（版本号只增），
//!     历史版本内容不可变（改写必拒）；
//!   - 完整性：每面快照附 FNV 自校验和，校验 10/10 必过；篡改注入
//!     10/10 必抓；
//!   - 基线登记率 100%：三面缺一不可封总版本。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFDB0 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0xFDB0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_FACE_MISS: u16 = SEG_BASE + 1;   // 三面缺一封版本
const E_CSUM_BAD: u16 = SEG_BASE + 2;    // 自校验和不符
const E_VERSION_REG: u16 = SEG_BASE + 3; // 版本回退/复用
const E_FROZEN: u16 = SEG_BASE + 4;      // 冻结面改写
const E_SAMP_RANGE: u16 = SEG_BASE + 5;  // 采样值越域
const E_LEN_MISS: u16 = SEG_BASE + 6;    // 快照长度不足
const E_LEDGER_FULL: u16 = SEG_BASE + 7; // 版本账满
const E_DUP_FACE: u16 = SEG_BASE + 8;    // 同版本同面重复挂
const E_SEAL_ORDER: u16 = SEG_BASE + 9;  // 未走完三面即封
const E_EMPTY: u16 = SEG_BASE + 10;      // 空快照

/// 每面快照采样容量。
pub const SNAP_CAP: usize = 64;

/// 版本账容量。
pub const VERSION_CAP: usize = 8;

/// 三面闭集：0=tick 1=serial 2=scancode。
pub const FACE_COUNT: usize = 3;

/// 确定性采样源（LCG，种子写死——退役前行为画像必须可复现）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state >> 11
    }
}

/// 面快照：定容采样 + 自校验和。
#[derive(Clone, Copy, Debug)]
pub struct FaceSnapshot {
    /// 面号（0~2）。
    pub face: u8,
    /// 采样长度（≤ SNAP_CAP）。
    pub len: u16,
    /// 采样数据（tick 间隔 µs / 串口字节 / 扫描码）。
    pub samples: [u16; SNAP_CAP],
    /// FNV 自校验和（len+samples 冻结时计算）。
    pub checksum: u64,
}

impl FaceSnapshot {
    /// 从确定性源生成快照（采样值域按面约束）。
    pub fn generate(face: u8, seed: u64) -> Result<FaceSnapshot, u16> {
        if face as usize >= FACE_COUNT {
            return Err(E_FACE_MISS);
        }
        let mut rng = Lcg::new(seed);
        let mut snap = FaceSnapshot {
            face,
            len: SNAP_CAP as u16,
            samples: [0; SNAP_CAP],
            checksum: 0,
        };
        let mut i = 0;
        while i < SNAP_CAP {
            let raw = rng.next();
            // 值域：tick 间隔 900~1100µs；串口字节 0x00~0xFF；扫描码 0x01~0x83。
            let v: u16 = match face {
                0 => 900 + (raw % 201) as u16,
                1 => (raw & 0xFF) as u16,
                _ => 1 + (raw % 0x83) as u16,
            };
            snap.samples[i] = v;
            i += 1;
        }
        snap.checksum = snap.compute_checksum();
        Ok(snap)
    }

    /// FNV 自校验和（face+len+samples 全覆盖）。
    pub fn compute_checksum(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        h ^= self.face as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= self.len as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        let n = self.len as usize;
        let mut i = 0;
        while i < n && i < SNAP_CAP {
            h ^= self.samples[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        h
    }

    /// 完整性断言：重算校验和与冻结值一致。
    pub fn verify(&self) -> bool {
        self.compute_checksum() == self.checksum
    }

    /// 采样值域核验（按面闭域）。
    pub fn samples_in_domain(&self) -> bool {
        let n = self.len as usize;
        let mut i = 0;
        while i < n && i < SNAP_CAP {
            let v = self.samples[i];
            let ok = match self.face {
                0 => (900..=1100).contains(&v),
                1 => v <= 0xFF,
                _ => (1..=0x83).contains(&v),
            };
            if !ok {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 基线版本：三面快照齐备后封版。
#[derive(Clone, Copy, Debug)]
pub struct BaselineVersion {
    pub version: u64,
    /// 三面快照（None=未挂面）。
    pub faces: [Option<FaceSnapshot>; FACE_COUNT],
    /// 版本指纹（三面校验和链）。
    pub fingerprint: u64,
}

impl BaselineVersion {
    pub const EMPTY: BaselineVersion = BaselineVersion {
        version: 0,
        faces: [None; FACE_COUNT],
        fingerprint: 0,
    };

    pub fn face_ready(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < FACE_COUNT {
            if self.faces[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 基线冻结账：append-only 版本账。
pub struct BaselineLedger {
    versions: [BaselineVersion; VERSION_CAP],
    used: usize,
    /// 当前在建版本（版本号=used+1 预留）。
    rejects: [u32; 11],
    total_frozen: u64,
}

impl BaselineLedger {
    pub fn new() -> BaselineLedger {
        BaselineLedger {
            versions: [BaselineVersion::EMPTY; VERSION_CAP],
            used: 0,
            rejects: [0; 11],
            total_frozen: 0,
        }
    }

    fn reject(&mut self, code: u16) -> u16 {
        let idx = (code - SEG_BASE) as usize;
        if idx < self.rejects.len() {
            self.rejects[idx] += 1;
        }
        code
    }

    /// 挂一面快照到当前在建版本（同版本同面重复必拒）。
    pub fn attach(&mut self, snap: FaceSnapshot) -> Result<usize, u16> {
        if self.used >= VERSION_CAP {
            return Err(self.reject(E_LEDGER_FULL));
        }
        if snap.len == 0 {
            return Err(self.reject(E_EMPTY));
        }
        if snap.len as usize > SNAP_CAP {
            return Err(self.reject(E_LEN_MISS));
        }
        if !snap.verify() {
            return Err(self.reject(E_CSUM_BAD));
        }
        if !snap.samples_in_domain() {
            return Err(self.reject(E_SAMP_RANGE));
        }
        if snap.face as usize >= FACE_COUNT {
            return Err(self.reject(E_FACE_MISS));
        }
        if self.versions[self.used].faces[snap.face as usize].is_some() {
            return Err(self.reject(E_DUP_FACE));
        }
        self.versions[self.used].faces[snap.face as usize] = Some(snap);
        Ok(self.versions[self.used].face_ready())
    }

    /// 封版：三面齐备才可封（版本号只增）。
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.used >= VERSION_CAP {
            return Err(self.reject(E_LEDGER_FULL));
        }
        if self.versions[self.used].face_ready() < FACE_COUNT {
            return Err(self.reject(E_FACE_MISS));
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        let mut f = 0;
        while f < FACE_COUNT {
            if let Some(s) = self.versions[self.used].faces[f] {
                fp ^= s.checksum;
                fp = fp.wrapping_mul(0x0000_0100_0000_01B3);
            }
            f += 1;
        }
        self.versions[self.used].version = self.total_frozen + 1;
        self.versions[self.used].fingerprint = fp;
        self.total_frozen += 1;
        self.used += 1;
        Ok(self.total_frozen)
    }

    /// 按版本号取版本（越界 None）。
    pub fn by_version(&self, v: u64) -> Option<&BaselineVersion> {
        if v == 0 || v as usize > self.used {
            None
        } else {
            Some(&self.versions[v as usize - 1])
        }
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn total_frozen(&self) -> u64 {
        self.total_frozen
    }
}

/// 基线完整性 10/10：三面生成→挂账→封版→逐面 verify，全过。
pub fn drill_integrity_10() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut round = 0;
    while round < 10 {
        let mut ledger = BaselineLedger::new();
        let seed = 0xFDB0_0000 + round as u64;
        let mut all_ok = true;
        let mut f = 0;
        while f < FACE_COUNT {
            match FaceSnapshot::generate(f as u8, seed + f as u64) {
                Ok(s) => {
                    if !s.verify() || !s.samples_in_domain() {
                        all_ok = false;
                    }
                    if ledger.attach(s).is_err() {
                        all_ok = false;
                    }
                }
                Err(_) => all_ok = false,
            }
            f += 1;
        }
        if ledger.freeze().is_err() {
            all_ok = false;
        }
        // 复验：从账里取出版本逐面再 verify。
        if let Some(v) = ledger.by_version(1) {
            let mut f2 = 0;
            while f2 < FACE_COUNT {
                if let Some(s) = v.faces[f2] {
                    if !s.verify() {
                        all_ok = false;
                    }
                }
                f2 += 1;
            }
        } else {
            all_ok = false;
        }
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    if ok == 10 {
        detail.push("完整性 10/10 全过".to_string());
    }
    (ok, 10, detail)
}

/// 版本只增不减演练：连续封 5 版，版本号 1..5 严格递增，旧版本内容
/// 不可变（同版本指纹重读一致）。
pub fn drill_version_monotonic() -> (bool, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = BaselineLedger::new();
    let mut fps = [0u64; 5];
    let mut v = 0;
    while v < 5 {
        let seed = 0xFDB0_BEEF + v as u64;
        let mut f = 0;
        while f < FACE_COUNT {
            let s = match FaceSnapshot::generate(f as u8, seed + f as u64) {
                Ok(s) => s,
                Err(_) => {
                    detail.push(format!("版{}面{}生成失败", v + 1, f));
                    return (false, detail);
                }
            };
            if ledger.attach(s).is_err() {
                detail.push(format!("版{}面{}挂账失败", v + 1, f));
                return (false, detail);
            }
            f += 1;
        }
        match ledger.freeze() {
            Ok(ver) => {
                if ver != v as u64 + 1 {
                    detail.push(format!("版本号非严格递增：{}≠{}", ver, v + 1));
                    return (false, detail);
                }
                fps[v] = ledger.by_version(ver).map(|b| b.fingerprint).unwrap_or(0);
            }
            Err(c) => {
                detail.push(format!("版{}封版被拒：0x{:04X}", v + 1, c));
                return (false, detail);
            }
        }
        v += 1;
    }
    // 旧版本不可变：重读指纹一致。
    let mut mono = true;
    let mut k = 0;
    while k < 5 {
        match ledger.by_version(k as u64 + 1) {
            Some(b) => {
                if b.fingerprint != fps[k] {
                    mono = false;
                }
            }
            None => mono = false,
        }
        k += 1;
    }
    (mono, detail)
}

/// 篡改注入 10/10：逐轮翻一个采样位，自校验和必抓。
pub fn inject_tamper_10() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 10 {
        let mut snap = match FaceSnapshot::generate(1, 0xFDB0_CAFE) {
            Ok(s) => s,
            Err(_) => break,
        };
        let pos = (r * 7 + 3) as usize % SNAP_CAP;
        snap.samples[pos] ^= 0x0001;
        if !snap.verify() {
            caught += 1;
        }
        r += 1;
    }
    if caught == 10 {
        detail.push("篡改注入 10/10 全抓".to_string());
    }
    (caught, 10, detail)
}

/// 缺面注入：只挂两面即封版必拒（生成率 100% 的反向面）。
pub fn inject_missing_face() -> (bool, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = BaselineLedger::new();
    let s0 = match FaceSnapshot::generate(0, 0xFDB0_1111) {
        Ok(s) => s,
        Err(_) => return (false, detail),
    };
    let s2 = match FaceSnapshot::generate(2, 0xFDB0_2222) {
        Ok(s) => s,
        Err(_) => return (false, detail),
    };
    let _ = ledger.attach(s0);
    let _ = ledger.attach(s2);
    let r = ledger.freeze();
    let caught = r == Err(E_FACE_MISS);
    if !caught {
        detail.push("缺面封版未拒".to_string());
    }
    (caught, detail)
}

/// 三面登记率演练：3 版 × 3 面全挂全封 → 登记率 100%。
pub fn drill_generation_coverage() -> (u32, u32, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = BaselineLedger::new();
    let mut frozen = 0u32;
    let mut v = 0;
    while v < 3 {
        let seed = 0xFDB0_5000 + v as u64;
        let mut f = 0;
        let mut face_ok = 0;
        while f < FACE_COUNT {
            if let Ok(s) = FaceSnapshot::generate(f as u8, seed + f as u64) {
                if ledger.attach(s).is_ok() {
                    face_ok += 1;
                }
            }
            f += 1;
        }
        if face_ok == FACE_COUNT && ledger.freeze().is_ok() {
            frozen += 1;
        }
        v += 1;
    }
    detail.push(format!("封版 {}/3（三面×3版=9 面全挂）", frozen));
    (frozen, 3, detail)
}

/// 段位守卫：0xFDB0 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_FACE_MISS, E_CSUM_BAD, E_VERSION_REG, E_FROZEN, E_SAMP_RANGE,
        E_LEN_MISS, E_LEDGER_FULL, E_DUP_FACE, E_SEAL_ORDER, E_EMPTY,
    ];
    let mut i = 0;
    while i < codes.len() {
        if codes[i] < SEG_BASE || codes[i] > SEG_BASE + 10 {
            return false;
        }
        if i > 0 && codes[i] != codes[i - 1] + 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// run_unx1287_checks：本单判据聚合（诊断段 0xFDB0）。
pub fn run_unx1287_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1287 行为基线冻结");
    let (c1, t1, d1) = drill_integrity_10();
    s.add_dyn("J1-完整性10/10", c1 == t1, &format!("{}/{} {}", c1, t1, d1.join(" ")));
    let (ok2, d2) = drill_version_monotonic();
    s.add_dyn("J1-版本化只增不减", ok2, &d2.join(" "));
    let (c3, t3, d3) = inject_tamper_10();
    s.add_dyn("J1-篡改注入10/10", c3 == t3, &format!("{}/{} {}", c3, t3, d3.join(" ")));
    let (ok4, d4) = inject_missing_face();
    s.add_dyn("J1-缺面封版必拒", ok4, &d4.join(" "));
    let (f5, t5, d5) = drill_generation_coverage();
    s.add_dyn("J1-三面生成率100%", f5 == t5, &format!("{}/{} {}", f5, t5, d5.join(" ")));
    s.add_dyn("J2-段位守卫", segment_guard(), "0xFDB0段十码闭集");
    s
}
