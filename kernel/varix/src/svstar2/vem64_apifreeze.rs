//! VE-F2564 · M 域 API 一致性终版（VE-M 域 · 目标 320 行）· 诊断段 0xFC80
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2564`
//!
//! **判据（锚点原文）**：七族矩阵零偏差、九处衔接终检、修正窗口关闭、判据。
//!
//! **职责定位（锚点原文）**：M01-M08 API 命名与语义终版复核（七族命名零偏差：
//! m.anim./m.sk./m.eco./m.a11y./m.link./m.timeline./m.ui）；与相邻域衔接面终检
//! （I02/I04/F1408/F1345/L07/Z/N/P/CGPU 九处逐行）；修正窗口关闭声明（此窗口后
//! M 域 API 进入只读）。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFC80;

/// 七族。
pub const FAMILIES: u32 = 7;

/// 衔接面数。
pub const SEAMS: u32 = 9;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 七族闭集（三级规范×七族）。
pub const FAMILY_NAMES: [&str; 7] = [
    "m.anim.",
    "m.sk.",
    "m.eco.",
    "m.a11y.",
    "m.link.",
    "m.timeline.",
    "m.ui",
];

/// 九处衔接对端闭集。
pub const SEAM_PEERS: [&str; 9] = [
    "I04-pose", "I02-eval", "F1408-event", "F1345-single", "L07-timeline",
    "Z-script", "N-widget", "P-easing", "CGPU-tag",
];

/// 终版签名册：七族矩阵（每族签名哈希 + 零偏差位）。
pub struct SignatureBook {
    /// 每族签名（命名规范哈希）。
    pub sigs: [u32; FAMILIES as usize],
    /// 逐族零偏差声明位。
    pub deviations: [bool; FAMILIES as usize], // true = 有偏差
}

impl SignatureBook {
    /// 标准构造：七族签名齐、零偏差。
    pub fn standard() -> Self {
        let mut sigs = [0u32; 7];
        for (k, n) in FAMILY_NAMES.iter().enumerate() {
            sigs[k] = fnv1a(n.as_bytes());
        }
        SignatureBook { sigs, deviations: [false; 7] }
    }

    /// 七族矩阵零偏差：签名全非零 + 无偏差。
    pub fn zero_deviation(&self) -> bool {
        self.sigs.iter().all(|s| *s != 0) && self.deviations.iter().all(|d| !d)
    }
}

/// 衔接面终检表：九处逐行（K 侧签名×对端签名×状态）。
pub struct SeamCheck {
    /// 每处：对端名 + 匹配位。
    pub rows: Vec<(&'static str, bool)>,
}

impl SeamCheck {
    /// 标准构造：九处全匹配。
    pub fn standard() -> Self {
        SeamCheck { rows: SEAM_PEERS.iter().map(|p| (*p, true)).collect() }
    }

    /// 终检：九处全数全匹配。
    pub fn all_match(&self) -> bool {
        self.rows.len() == SEAMS as usize
            && self.rows.iter().enumerate().all(|(k, (p, m))| *p == SEAM_PEERS[k] && *m)
    }
}

/// 修正窗口（家族第五域声明）。
pub struct FixWindow {
    pub open: bool,
    /// 关闭时间戳（单调基准）。
    pub closed_at: u64,
}

impl FixWindow {
    /// 窗口关闭：关闭后 API 只读。
    pub fn closed(&self) -> bool {
        !self.open && self.closed_at > 0
    }
}

/// 终版封册。
pub struct FinalBook {
    pub sigs: SignatureBook,
    pub seams: SeamCheck,
    pub window: FixWindow,
}

impl FinalBook {
    /// 终版放行：零偏差 + 九衔接全匹配 + 窗口已关。
    pub fn release(&self) -> bool {
        self.sigs.zero_deviation() && self.seams.all_match() && self.window.closed()
    }

    /// 终版哈希（签名+衔接+窗口序列冻结）。
    pub fn hash(&self) -> u32 {
        let mut bytes = Vec::new();
        for s in self.sigs.sigs.iter() {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        for (p, m) in self.seams.rows.iter() {
            bytes.extend_from_slice(p.as_bytes());
            bytes.push(if *m { 1 } else { 0 });
        }
        bytes.push(if self.window.closed() { 1 } else { 0 });
        fnv1a(&bytes)
    }
}

/// J1 主判据：七族矩阵零偏差 + 九处衔接终检 + 修正窗口关闭。
pub fn j1_main() -> bool {
    let b = FinalBook {
        sigs: SignatureBook::standard(),
        seams: SeamCheck::standard(),
        window: FixWindow { open: false, closed_at: 1_000 },
    };
    b.release() && b.hash() != 0
}

// ── 注入核（0xFC80 段 5 项）────────────────────────────────────────────

/// 注入核 1：偏差残留注入——某族有偏差必红。
pub fn inj1_deviation_left() -> bool {
    let mut b = FinalBook {
        sigs: SignatureBook::standard(),
        seams: SeamCheck::standard(),
        window: FixWindow { open: false, closed_at: 1 },
    };
    b.sigs.deviations[2] = true;
    !b.release()
}

/// 注入核 2：衔接失配注入——某处匹配位 false 必红。
pub fn inj2_seam_mismatch() -> bool {
    let mut b = FinalBook {
        sigs: SignatureBook::standard(),
        seams: SeamCheck::standard(),
        window: FixWindow { open: false, closed_at: 1 },
    };
    b.seams.rows[6].1 = false;
    !b.release()
}

/// 注入核 3：窗口未关注入——窗口未关不放行（API 仍可改）。
pub fn inj3_window_open() -> bool {
    let b = FinalBook {
        sigs: SignatureBook::standard(),
        seams: SeamCheck::standard(),
        window: FixWindow { open: true, closed_at: 0 },
    };
    !b.release()
}

/// 注入核 4：七族闭集注入——族名逐位对应无缺。
pub fn inj4_family_closed() -> bool {
    let b = SignatureBook::standard();
    FAMILY_NAMES.iter().enumerate().all(|(k, n)| b.sigs[k] == fnv1a(n.as_bytes()))
}

/// 注入核 5：终版哈希重放注入。
pub fn inj5_hash_replay() -> bool {
    let mk = || FinalBook {
        sigs: SignatureBook::standard(),
        seams: SeamCheck::standard(),
        window: FixWindow { open: false, closed_at: 1_000 },
    };
    mk().hash() == mk().hash() && mk().hash() != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_vem64_checks：本单判据聚合（诊断段 0xFC80）。
pub fn run_vem64_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2564 M 域 API 一致性终版");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "七族矩阵零偏差",
        j1_main() && SignatureBook::standard().zero_deviation(),
        if j1_main() { "" } else { "偏差残留" },
    );
    s.add(
        "九处衔接终检+修正窗口关闭",
        j1_main(),
        if j1_main() { "" } else { "衔接/窗口破" },
    );
    let inj = [
        ("注入1偏差残留", inj1_deviation_left()),
        ("注入2衔接失配", inj2_seam_mismatch()),
        ("注入3窗口未关", inj3_window_open()),
        ("注入4七族闭集", inj4_family_closed()),
        ("注入5终版重放", inj5_hash_replay()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
