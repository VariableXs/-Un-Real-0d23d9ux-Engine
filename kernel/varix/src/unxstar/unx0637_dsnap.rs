//! unx0637_dsnap —— UNX-F0637 域终态快照（复扫零漂移）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0637 · 域终态快照（复扫零漂移） · 目标 ≥220 行
//! 判据原文摘录：域快照复扫（六段+12 包+33 批册三面指纹全等 3/3 轮），
//!   注入漂移检出 100%。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 三面指纹：六段面 / 12 包面 / 33 批册面，各一面 FNV-1a 64 指纹，
//!      面指纹 = 该面全部账目字节的确定性哈希。
//!   2. 复扫零漂移判据：3/3 轮复扫逐面指纹与首拍全等（相等比对，
//!      不做模糊容差）。
//!   3. 注入漂移检出：三面任一面注入一字节漂移，对应轮次必须红并
//!      定位面号。
//!   4. 快照定容：首拍落账后快照只读，任何"改快照再复扫"入口走
//!      注入面（显式标记）而非隐改。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x5160 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x5160 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x5160;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x516F;

/// XcCode：面号越界。
pub const XC_BAD_FACE: u16 = SEG_BASE + 1; // 0x5161 三面闭集外
/// XcCode：复扫漂移。
pub const XC_DRIFT: u16 = SEG_BASE + 2; // 0x5162 指纹不等
/// XcCode：快照未落首拍。
pub const XC_NO_BASE: u16 = SEG_BASE + 3; // 0x5163 未落首拍先复扫
/// XcCode：复扫轮数不足。
pub const XC_ROUND_SHORT: u16 = SEG_BASE + 4; // 0x5164 <3/3 轮
/// XcCode：面账目为空。
pub const XC_EMPTY_FACE: u16 = SEG_BASE + 5; // 0x5165 空面拒收
/// XcCode：快照轮账重复。
pub const XC_DUP_ROUND: u16 = SEG_BASE + 6; // 0x5166 一轮一拍

/// 三面数。
pub const FACE_N: usize = 3;
/// 复扫轮数（判据原文 3/3）。
pub const ROUND_N: usize = 3;
/// 包面包数。
pub const PKG_N: usize = 12;
/// 批册批数。
pub const BATCH_N: usize = 33;

/// FNV-1a 64。
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 三面账目源（快照的原始素材，可确定性生成）。
#[derive(Clone, Debug)]
pub struct DomainFaces {
    /// 六段面（段名:载荷 行集）。
    pub six_sections: Vec<String>,
    /// 12 包面（包账行）。
    pub pkg_rows: Vec<String>,
    /// 33 批册面（批账行）。
    pub batch_rows: Vec<String>,
}

impl DomainFaces {
    /// 标准域账面（确定性生成）。
    pub fn standard() -> Self {
        let mut six = Vec::new();
        let names = ["概括", "对冲", "红线", "覆盖", "遗留", "建议"];
        for (i, n) in names.iter().enumerate() {
            six.push(alloc::format!("段{} {} 载荷行数{}", i, n, (i + 1) * 11));
        }
        let mut pkgs = Vec::new();
        for i in 0..PKG_N {
            pkgs.push(alloc::format!("包{} 判据F0{} 行数{}", i, 349 + i * 25, i * 7 + 3));
        }
        let mut batches = Vec::new();
        for i in 0..BATCH_N {
            batches.push(alloc::format!("批{} 条目20 行数{}", i, 300 + i * 9));
        }
        DomainFaces {
            six_sections: six,
            pkg_rows: pkgs,
            batch_rows: batches,
        }
    }

    /// 面字节集（面号 0..3：六段/包/批册）。
    pub fn face_bytes(&self, face: usize) -> Result<Vec<u8>, u16> {
        let mut buf = Vec::new();
        match face {
            0 => {
                if self.six_sections.is_empty() {
                    return Err(XC_EMPTY_FACE);
                }
                for l in &self.six_sections {
                    buf.extend_from_slice(l.as_bytes());
                }
            }
            1 => {
                if self.pkg_rows.is_empty() {
                    return Err(XC_EMPTY_FACE);
                }
                for l in &self.pkg_rows {
                    buf.extend_from_slice(l.as_bytes());
                }
            }
            2 => {
                if self.batch_rows.is_empty() {
                    return Err(XC_EMPTY_FACE);
                }
                for l in &self.batch_rows {
                    buf.extend_from_slice(l.as_bytes());
                }
            }
            _ => return Err(XC_BAD_FACE),
        }
        Ok(buf)
    }
}

/// 域终态快照：首拍三面指纹 + 复扫轮账。
#[derive(Clone, Debug)]
pub struct DomainSnap {
    /// 首拍三面指纹。
    base: Option<[u64; FACE_N]>,
    /// 复扫轮账（轮号 → 三面指纹，逐轮追加）。
    rounds: Vec<(usize, [u64; FACE_N])>,
}

impl DomainSnap {
    /// 空快照。
    pub fn new() -> Self {
        DomainSnap {
            base: None,
            rounds: Vec::new(),
        }
    }

    /// 落首拍（三面指纹落账；重落拒收）。
    pub fn take_base(&mut self, faces: &DomainFaces) -> Result<[u64; FACE_N], u16> {
        if self.base.is_some() {
            return Err(XC_DUP_ROUND);
        }
        let mut fps = [0u64; FACE_N];
        for f in 0..FACE_N {
            fps[f] = fnv64(&faces.face_bytes(f)?);
        }
        self.base = Some(fps);
        Ok(fps)
    }

    /// 复扫一轮：逐面重算并对比首拍；漂移即红并定位面号。
    pub fn rescan(&mut self, faces: &DomainFaces) -> Result<usize, u16> {
        let base = match self.base {
            Some(b) => b,
            None => return Err(XC_NO_BASE),
        };
        let round_no = self.rounds.len();
        let mut fps = [0u64; FACE_N];
        for f in 0..FACE_N {
            let fp = fnv64(&faces.face_bytes(f)?);
            if fp != base[f] {
                return Err(XC_DRIFT);
            }
            fps[f] = fp;
        }
        self.rounds.push((round_no, fps));
        Ok(round_no)
    }

    /// 复扫零漂移总核：3/3 轮全跑且逐轮三面与首拍全等。
    pub fn zero_drift(&self) -> Result<usize, u16> {
        let base = match self.base {
            Some(b) => b,
            None => return Err(XC_NO_BASE),
        };
        if self.rounds.len() != ROUND_N {
            return Err(XC_ROUND_SHORT);
        }
        for (i, (rn, fps)) in self.rounds.iter().enumerate() {
            if *rn != i {
                return Err(XC_ROUND_SHORT);
            }
            for f in 0..FACE_N {
                if fps[f] != base[f] {
                    return Err(XC_DRIFT);
                }
            }
        }
        Ok(ROUND_N)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "域终态快照 · 首拍={} · 复扫轮 {}/{}",
            self.base.is_some(),
            self.rounds.len(),
            ROUND_N
        );
        if let Some(b) = &self.base {
            for f in 0..FACE_N {
                s.push_str(&alloc::format!("\n  面{} {:016x}", f, b[f]));
            }
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv64(self.report().as_bytes())
    }
}

/// 标准快照流程：落首拍 + 3 轮复扫 + 零漂移总核。
pub fn run_snapshot() -> Result<DomainSnap, u16> {
    let faces = DomainFaces::standard();
    let mut snap = DomainSnap::new();
    snap.take_base(&faces)?;
    for _ in 0..ROUND_N {
        snap.rescan(&faces)?;
    }
    snap.zero_drift()?;
    Ok(snap)
}

/// 自例程：主路径（3/3 轮零漂移）+ 错误路径（漂移注入/未落首拍/轮不足/面越界）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let snap = run_snapshot()?;
    if snap.zero_drift()? != ROUND_N {
        return Err(XC_ROUND_SHORT);
    }
    if snap.fingerprint() == 0 {
        return Err(XC_DRIFT);
    }

    // 错误路径 1：六段面注入漂移（改一行）→ 当轮红。
    let mut faces1 = DomainFaces::standard();
    faces1.six_sections[2] = alloc::format!("段2 红线 载荷行数999");
    let mut s1 = DomainSnap::new();
    s1.take_base(&DomainFaces::standard())?;
    if s1.rescan(&faces1) != Err(XC_DRIFT) {
        return Err(XC_DRIFT);
    }
    // 错误路径 2：包面注入漂移。
    let mut faces2 = DomainFaces::standard();
    faces2.pkg_rows[5] = alloc::format!("包5 篡改");
    let mut s2 = DomainSnap::new();
    s2.take_base(&DomainFaces::standard())?;
    if s2.rescan(&faces2) != Err(XC_DRIFT) {
        return Err(XC_DRIFT);
    }
    // 错误路径 3：批册面注入漂移。
    let mut faces3 = DomainFaces::standard();
    faces3.batch_rows[0] = alloc::format!("批0 条目21 行数300");
    let mut s3 = DomainSnap::new();
    s3.take_base(&DomainFaces::standard())?;
    if s3.rescan(&faces3) != Err(XC_DRIFT) {
        return Err(XC_DRIFT);
    }
    // 错误路径 4：未落首拍先复扫。
    let mut s4 = DomainSnap::new();
    if s4.rescan(&DomainFaces::standard()) != Err(XC_NO_BASE) {
        return Err(XC_NO_BASE);
    }
    // 错误路径 5：复扫轮数不足。
    let mut s5 = DomainSnap::new();
    let f = DomainFaces::standard();
    s5.take_base(&f)?;
    s5.rescan(&f)?;
    if s5.zero_drift() != Err(XC_ROUND_SHORT) {
        return Err(XC_ROUND_SHORT);
    }
    // 错误路径 6：面号越界。
    let mut s6 = DomainSnap::new();
    if s6.take_base(&DomainFaces::standard()).is_ok() {
        // 面越界走 face_bytes 错误路径。
    }
    let f6 = DomainFaces::standard();
    if f6.face_bytes(FACE_N).is_err() {
        // 预期错误路径。
    } else {
        return Err(XC_BAD_FACE);
    }
    // 错误路径 7：重落首拍拒收。
    let mut s7 = run_snapshot()?;
    if s7.take_base(&f6) != Err(XC_DUP_ROUND) {
        return Err(XC_DUP_ROUND);
    }
    Ok(0)
}
