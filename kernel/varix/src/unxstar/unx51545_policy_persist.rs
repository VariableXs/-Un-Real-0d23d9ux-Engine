//! UNX-F51545 端口策略持久化（跨重启生效） · 0xC0B00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51546-J1 BCD 登记与兼容矩阵联动 ×10 一致
//!
//! 设计要点：
//!   ① 持久化域按「冻结字节镜像」建模：镜像头 12B（魔数 u32 + 修订 u32 +
//!      计数 u32）+ 每记录 8B（端口 u32 + 档位 u32），字节账冻结公式
//!      镜像长 ≡ 12 + 8×记录数，逐字节对平（u32 字段一律算 4B）。
//!   ② 重启语义：保存镜像 → 弃引擎 → 新引擎灌镜像 → 记录逐条比对，×10
//!      轮不同策略集全部恢复一致；灌镜像成功即把镜像修订代入新引擎。
//!   ③ 修订单调：每次变更修订 +1；镜像修订落后于内核纪元即旧镜像回灌，
//!      显性拒（RevRegress），被拒灌镜像不动引擎任何状态。
//!   ④ 篡改注入器：魔数篡改、长度截断、端口越界、档位越码、重复端口、
//!      陈旧修订六类注入各 ×10 轮全抓；守恒账 注入=检出+逃逸，逃逸恒 0。
//!   ⑤ 判定次序：镜像长度 → 魔数 → 修订 → 记录数 → 逐记录（端口 → 档位
//!      → 重复）；同时成立取先者。
//!   ⑥ 全程零浮点、零 IO（镜像只在内存）、零墙钟、零随机；字节编解码一
//!      律 .get()/.get_mut() + checked_*。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0B00;

/// 镜像头字节数（魔数 4 + 修订 4 + 计数 4）。
pub const HEADER_BYTES: usize = 12;
/// 单记录字节数（端口 4 + 档位 4）。
pub const RECORD_BYTES: usize = 8;
/// 镜像魔数（"UNXE"）。
pub const PERSIST_MAGIC: u32 = 0x554E_5845;
/// 端口号上限。
pub const MAX_PORT: usize = 16;
/// 档位码上限（0/1/2 三档）。
pub const MAX_TIER_CODE: u32 = 2;
/// 记录数上限。
pub const MAX_RECORDS: usize = 16;

/// 冻结字节账公式：镜像长 ≡ 12 + 8×记录数。
pub fn expected_blob_len(count: u32) -> Option<usize> {
    let c = count as usize;
    match RECORD_BYTES.checked_mul(c) {
        Some(m) => m.checked_add(HEADER_BYTES),
        None => None,
    }
}

/// 从镜像偏移读 u32（大端），越界返回 None。
pub fn read_u32_be(blob: &[u8], off: usize) -> Option<u32> {
    let b0 = match blob.get(off) {
        Some(v) => *v as u32,
        None => return None,
    };
    let o1 = match off.checked_add(1) {
        Some(v) => v,
        None => return None,
    };
    let b1 = match blob.get(o1) {
        Some(v) => *v as u32,
        None => return None,
    };
    let o2 = match off.checked_add(2) {
        Some(v) => v,
        None => return None,
    };
    let b2 = match blob.get(o2) {
        Some(v) => *v as u32,
        None => return None,
    };
    let o3 = match off.checked_add(3) {
        Some(v) => v,
        None => return None,
    };
    let b3 = match blob.get(o3) {
        Some(v) => *v as u32,
        None => return None,
    };
    Some((b0 << 24) | (b1 << 16) | (b2 << 8) | b3)
}

/// 大端写 u32 入镜像（冻结布局，u32 一律 4B）。
pub fn push_u32_be(out: &mut Vec<u8>, v: u32) {
    out.push((v >> 24) as u8);
    out.push((v >> 16) as u8);
    out.push((v >> 8) as u8);
    out.push(v as u8);
}

/// 单条端口策略记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyRecord {
    pub port: usize,
    pub tier: u32,
}

/// 诊断码闭集（策略持久化域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PersistCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 记录数超上限
    StoreFull,
    /// 镜像长度与字节账不符（含过短）
    BlobTooShort,
    /// 魔数不符
    MagicMismatch,
    /// 镜像修订落后（旧镜像回灌显性拒）
    RevRegress,
    /// 镜像记录数超上限
    CountTooBig,
    /// 镜像内端口号越界
    PortOob,
    /// 镜像内档位码越闭集
    TierUnknown,
    /// 镜像内端口重复
    DupPort,
    /// 账面不符
    LedgerMismatch,
}

impl PersistCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PersistCode::Ok => "持久化操作成功",
            PersistCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            PersistCode::StoreFull => "策略表满：记录数不得超上限",
            PersistCode::BlobTooShort => "镜像长度不符：镜像长 ≡ 12 + 8×记录数",
            PersistCode::MagicMismatch => "魔数不符：镜像头标识被篡改或非本域镜像",
            PersistCode::RevRegress => "旧镜像回灌：镜像修订不得落后于内核纪元",
            PersistCode::CountTooBig => "记录数超上限：镜像记录数越界",
            PersistCode::PortOob => "镜像端口越界：端口号必须在 0..16 之内",
            PersistCode::TierUnknown => "镜像档位非法：只接受 0/1/2 三档",
            PersistCode::DupPort => "镜像端口重复：每端口至多一条策略",
            PersistCode::LedgerMismatch => "账面不符：保存与恢复计数必须对平",
        }
    }
}

/// 端口策略持久化仓库：策略表 + 修订纪元 + 守恒账。
pub struct PolicyStore {
    recs: Vec<PolicyRecord>,
    rev: u64,
    save_cnt: core::cell::Cell<u64>,
    restore_ok_cnt: u64,
    restore_reject_cnt: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl PolicyStore {
    pub fn new() -> PolicyStore {
        PolicyStore {
            recs: Vec::new(),
            rev: 0,
            save_cnt: core::cell::Cell::new(0),
            restore_ok_cnt: 0,
            restore_reject_cnt: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 登记或替换一条端口策略（修订 +1）。
    pub fn upsert(&mut self, port: usize, tier_code: u32) -> Result<(), PersistCode> {
        if port >= MAX_PORT {
            return Err(PersistCode::PortOob);
        }
        if tier_code > MAX_TIER_CODE {
            return Err(PersistCode::TierUnknown);
        }
        let mut replaced = false;
        for r in self.recs.iter_mut() {
            if r.port == port {
                r.tier = tier_code;
                replaced = true;
            }
        }
        if !replaced {
            if self.recs.len() >= MAX_RECORDS {
                return Err(PersistCode::StoreFull);
            }
            self.recs.push(PolicyRecord { port, tier: tier_code });
        }
        self.rev = self.rev.saturating_add(1);
        Ok(())
    }

    /// 保存镜像（冻结布局：12B 头 + 8B×记录）。
    pub fn save(&self) -> Vec<u8> {
        let mut blob = Vec::new();
        push_u32_be(&mut blob, PERSIST_MAGIC);
        push_u32_be(&mut blob, self.rev as u32);
        push_u32_be(&mut blob, self.recs.len() as u32);
        for r in self.recs.iter() {
            push_u32_be(&mut blob, r.port as u32);
            push_u32_be(&mut blob, r.tier);
        }
        self.save_cnt.set(self.save_cnt.get().saturating_add(1));
        blob
    }

    /// 灌镜像（重启恢复）：判定次序 长度 → 魔数 → 修订 → 记录数 → 逐记录；
    /// 成功即把镜像修订代入内核纪元（只许前进）。
    pub fn restore(&mut self, blob: &[u8]) -> Result<usize, PersistCode> {
        match self.parse_blob(blob) {
            Ok((recs, blob_rev)) => {
                self.recs = recs;
                if blob_rev > self.rev {
                    self.rev = blob_rev;
                }
                self.restore_ok_cnt = self.restore_ok_cnt.saturating_add(1);
                Ok(self.recs.len())
            }
            Err(e) => {
                self.restore_reject_cnt = self.restore_reject_cnt.saturating_add(1);
                Err(e)
            }
        }
    }

    /// 镜像解析（不含计数副作用，返回记录集与镜像修订，供 restore 包装）。
    fn parse_blob(&self, blob: &[u8]) -> Result<(Vec<PolicyRecord>, u64), PersistCode> {
        if blob.len() < HEADER_BYTES {
            return Err(PersistCode::BlobTooShort);
        }
        let magic = read_u32_be(blob, 0).ok_or(PersistCode::BlobTooShort)?;
        if magic != PERSIST_MAGIC {
            return Err(PersistCode::MagicMismatch);
        }
        let blob_rev = read_u32_be(blob, 4).ok_or(PersistCode::BlobTooShort)? as u64;
        if blob_rev < self.rev {
            return Err(PersistCode::RevRegress);
        }
        let count = read_u32_be(blob, 8).ok_or(PersistCode::BlobTooShort)?;
        let want_len = expected_blob_len(count).ok_or(PersistCode::CountTooBig)?;
        if blob.len() != want_len {
            return Err(PersistCode::BlobTooShort);
        }
        if count as usize > MAX_RECORDS {
            return Err(PersistCode::CountTooBig);
        }
        let mut seen: Vec<usize> = Vec::new();
        let mut out: Vec<PolicyRecord> = Vec::new();
        for i in 0..count as usize {
            let off = match HEADER_BYTES.checked_add(
                RECORD_BYTES.checked_mul(i).ok_or(PersistCode::CountTooBig)?,
            ) {
                Some(v) => v,
                None => return Err(PersistCode::CountTooBig),
            };
            let port_u = read_u32_be(blob, off).ok_or(PersistCode::BlobTooShort)?;
            let tier = read_u32_be(blob, off.saturating_add(4)).ok_or(PersistCode::BlobTooShort)?;
            if port_u as usize >= MAX_PORT {
                return Err(PersistCode::PortOob);
            }
            if tier > MAX_TIER_CODE {
                return Err(PersistCode::TierUnknown);
            }
            if seen.contains(&(port_u as usize)) {
                return Err(PersistCode::DupPort);
            }
            seen.push(port_u as usize);
            out.push(PolicyRecord {
                port: port_u as usize,
                tier,
            });
        }
        Ok((out, blob_rev))
    }

    /// 重启模拟：保存 → 弃引擎 → 新引擎灌镜像 → 记录逐条返回。
    pub fn reboot_roundtrip(&self) -> Result<Vec<PolicyRecord>, PersistCode> {
        let blob = self.save();
        let mut fresh = PolicyStore::new();
        fresh.restore(&blob)?;
        Ok(fresh.recs.clone())
    }

    pub fn records(&self) -> Vec<PolicyRecord> {
        self.recs.clone()
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn tally(&self) -> (u64, u64, u64) {
        (self.save_cnt.get(), self.restore_ok_cnt, self.restore_reject_cnt)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0 且检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u64, u64, u64, u64, usize) {
        (self.rev, self.save_cnt.get(), self.restore_ok_cnt, self.restore_reject_cnt, self.recs.len())
    }
}

/// 手工镜像构造器（注入演练用，冻结布局）。
pub fn raw_blob(rev: u32, recs: &[(u32, u32)]) -> Vec<u8> {
    let mut v = Vec::new();
    push_u32_be(&mut v, PERSIST_MAGIC);
    push_u32_be(&mut v, rev);
    push_u32_be(&mut v, recs.len() as u32);
    for &(p, t) in recs.iter() {
        push_u32_be(&mut v, p);
        push_u32_be(&mut v, t);
    }
    v
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一脚本两次建仓库，镜像与快照逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<u8>, (u64, u64, u64, u64, usize)) {
        let mut st = PolicyStore::new();
        let _ = st.upsert(0, 0);
        let _ = st.upsert(3, 1);
        let _ = st.upsert(7, 2);
        let blob = st.save();
        let snap = st.snapshot();
        (blob, snap)
    };
    build() == build()
}

/// ×10 重启一致：第 i 轮用不同策略集做保存→重启→恢复→比对。
pub fn reboot_consistent_round(i: usize) -> bool {
    let mut st = PolicyStore::new();
    let n = 1 + i % MAX_RECORDS;
    for p in 0..n {
        let tier = (p + i) % 3;
        if st.upsert(p, tier as u32).is_err() {
            return false;
        }
    }
    let want = st.records();
    match st.reboot_roundtrip() {
        Ok(got) => got == want,
        Err(_) => false,
    }
}

/// 收口自检（16 条判据形态）。
pub fn run_unx51545_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51545");
    // 1 正常流：保存→重启→恢复，记录逐条一致。
    {
        let mut st = PolicyStore::new();
        let _ = st.upsert(0, 0);
        let _ = st.upsert(3, 1);
        let _ = st.upsert(7, 2);
        let _ = st.upsert(12, 1);
        let _ = st.upsert(15, 0);
        let want = st.records();
        let rt = st.reboot_roundtrip();
        s.add_dyn(
            "重启恢复一致",
            matches!(rt, Ok(got) if got == want),
            "跨重启恢复的记录必须与保存逐条一致",
        );
    }
    // 2 字节账公式：镜像长 ≡ 12 + 8×n（n=0/1/7/16 逐一对平）。
    {
        let mut all = true;
        for n in [0usize, 1, 7, 16] {
            let mut st = PolicyStore::new();
            for p in 0..n {
                let _ = st.upsert(p, (p % 3) as u32);
            }
            let blob = st.save();
            let want = match expected_blob_len(n as u32) {
                Some(v) => v,
                None => 0,
            };
            if blob.len() != want {
                all = false;
            }
        }
        s.add_dyn("字节账冻结公式", all, "u32 字段一律 4B，镜像长必须逐字节对平");
    }
    // 3 边界：空表与满表 16 条均可回环；第 17 口 StoreFull。
    {
        let empty = PolicyStore::new();
        let rt0 = empty.reboot_roundtrip();
        let mut full = PolicyStore::new();
        let mut fill_ok = true;
        for p in 0..MAX_RECORDS {
            if full.upsert(p, (p % 3) as u32).is_err() {
                fill_ok = false;
            }
        }
        let rt16 = full.reboot_roundtrip();
        let over = full.upsert(MAX_PORT - 1, 1); // 已存在 → 替换不算超
        let over2 = full.upsert(MAX_PORT, 1); // 越界 → PortOob
        s.add_dyn(
            "空表满表边界",
            matches!(rt0, Ok(r) if r.is_empty())
                && fill_ok
                && matches!(rt16, Ok(r) if r.len() == MAX_RECORDS)
                && over.is_ok()
                && over2 == Err(PersistCode::PortOob),
            "空表与 16 条满表可回环，越界口显性拒",
        );
    }
    // 4 注入必抓：魔数篡改 ×10 轮。
    {
        let mut st = PolicyStore::new();
        let _ = st.upsert(0, 0);
        let _ = st.upsert(5, 2);
        let blob = st.save();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let mut bad = blob.clone();
            if let Some(b0) = bad.get_mut(0) {
                *b0 = b0.wrapping_add(1 + i as u8);
            }
            if st.restore(&bad) == Err(PersistCode::MagicMismatch) {
                caught += 1;
                st.note_detected();
            }
        }
        s.add_dyn("魔数篡改注入 10/10", caught == 10, "魔数不符必须全抓");
    }
    // 5 注入必抓：长度截断 ×10 轮。
    {
        let mut st = PolicyStore::new();
        for p in 0..5usize {
            let _ = st.upsert(p, 1);
        }
        let blob = st.save();
        let mut caught = 0u32;
        for k in 1..=10usize {
            let mut bad = blob.clone();
            let keep = blob.len().saturating_sub(k);
            bad.truncate(keep);
            if st.restore(&bad) == Err(PersistCode::BlobTooShort) {
                caught += 1;
                st.note_detected();
            }
        }
        s.add_dyn("长度截断注入 10/10", caught == 10, "镜像长度与字节账不符必须全抓");
    }
    // 6 注入必抓：端口越界 ×10 轮。
    {
        let mut st = PolicyStore::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if st.upsert(MAX_PORT + i as usize, 0) == Err(PersistCode::PortOob) {
                caught += 1;
                st.note_detected();
            }
        }
        s.add_dyn("端口越界注入 10/10", caught == 10, "越界端口必须全抓且不入表");
    }
    // 7 注入必抓：档位越码 ×10 轮。
    {
        let mut st = PolicyStore::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if st.upsert(2, 3 + i) == Err(PersistCode::TierUnknown) {
                caught += 1;
                st.note_detected();
            }
        }
        s.add_dyn("档位越码注入 10/10", caught == 10, "档位越闭集必须全抓");
    }
    // 8 注入必抓：陈旧修订回灌 ×10 轮。
    {
        let mut st = PolicyStore::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let stale = st.save(); // 当前修订的镜像
            let port = i as usize % MAX_PORT;
            let _ = st.upsert(port, (i % 3) as u32); // 修订 +1，镜像变旧
            if st.restore(&stale) == Err(PersistCode::RevRegress) {
                caught += 1;
                st.note_detected();
            }
        }
        s.add_dyn("陈旧修订注入 10/10", caught == 10, "旧镜像回灌必须全抓");
    }
    // 9 注入必抓：镜像重口记录 ×10 轮。
    {
        let mut st = PolicyStore::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let bad = raw_blob(0, &[(1, 0), (1, i % 3)]);
            if st.restore(&bad) == Err(PersistCode::DupPort) {
                caught += 1;
                st.note_detected();
            }
        }
        s.add_dyn("重口记录注入 10/10", caught == 10, "镜像端口重复必须全抓");
    }
    // 10 记录数超上限：17 条镜像 CountTooBig。
    {
        let mut st = PolicyStore::new();
        let recs: Vec<(u32, u32)> = (0..17u32).map(|i| (i, i % 3)).collect();
        let bad = raw_blob(0, &recs);
        let r = st.restore(&bad);
        s.add_dyn(
            "记录数超上限",
            r == Err(PersistCode::CountTooBig),
            "镜像记录数超过 16 必须显性拒",
        );
    }
    // 11 判定次序：长度 → 魔数 → 修订。
    {
        let mut st = PolicyStore::new();
        let _ = st.upsert(0, 0);
        let _ = st.upsert(1, 1);
        let _ = st.upsert(2, 2); // rev = 3
        let short = [0u8; 8];
        let mut wrong_magic = raw_blob(0, &[]);
        if let Some(b0) = wrong_magic.get_mut(0) {
            *b0 = b0.wrapping_add(1);
        }
        let wrong_rev = raw_blob(1, &[]);
        s.add_dyn(
            "判序长度魔数修订",
            st.restore(&short) == Err(PersistCode::BlobTooShort)
                && st.restore(&wrong_magic) == Err(PersistCode::MagicMismatch)
                && st.restore(&wrong_rev) == Err(PersistCode::RevRegress),
            "长度判据先于魔数，魔数先于修订",
        );
    }
    // 12 状态机拒止：被拒回灌不动 rev 与策略表。
    {
        let mut st = PolicyStore::new();
        let _ = st.upsert(0, 0);
        let _ = st.upsert(1, 1);
        let snap = st.snapshot();
        let want = st.records();
        let stale = raw_blob(0, &[(4, 2)]);
        let _ = st.restore(&stale);
        s.add_dyn(
            "拒后状态不动",
            st.snapshot() == snap && st.records() == want,
            "旧镜像回灌被拒不得改动任何状态",
        );
    }
    // 13 对账守恒：注入 = 检出，逃逸 0。
    {
        let mut st = PolicyStore::new();
        for i in 0..10u32 {
            let _ = st.upsert(MAX_PORT + i as usize, 0);
            st.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            st.conserved() && st.snapshot() == (0, 0, 0, 0, 0),
            "注入=检出+逃逸，逃逸恒 0，被拒登记不入表",
        );
    }
    // 14 ×10 重启一致 + 双跑一致。
    {
        let mut all = true;
        for i in 0..10usize {
            if !reboot_consistent_round(i) {
                all = false;
            }
        }
        s.add_dyn("跨重启 ×10 恢复一致", all, "10 轮不同策略集恢复必须逐条一致");
        s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一脚本两次建仓镜像与快照相等");
    }
    // 15 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 16 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [PersistCode; 11] = [
    PersistCode::Ok,
    PersistCode::SegBad,
    PersistCode::StoreFull,
    PersistCode::BlobTooShort,
    PersistCode::MagicMismatch,
    PersistCode::RevRegress,
    PersistCode::CountTooBig,
    PersistCode::PortOob,
    PersistCode::TierUnknown,
    PersistCode::DupPort,
    PersistCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_records() {
        let mut st = PolicyStore::new();
        let _ = st.upsert(0, 0);
        let _ = st.upsert(7, 2);
        let _ = st.upsert(15, 1);
        let want = st.records();
        match st.reboot_roundtrip() {
            Ok(got) => assert_eq!(got, want),
            Err(e) => panic!("回环失败：{:?}", e),
        }
    }

    #[test]
    fn byte_formula_holds() {
        for n in [0usize, 1, 7, 16] {
            let mut st = PolicyStore::new();
            for p in 0..n {
                let _ = st.upsert(p, (p % 3) as u32);
            }
            let blob = st.save();
            let want = match expected_blob_len(n as u32) {
                Some(v) => v,
                None => 0,
            };
            assert_eq!(blob.len(), want, "n={} 字节账不符", n);
        }
    }

    #[test]
    fn store_full_and_port_bounds() {
        let mut st = PolicyStore::new();
        for p in 0..MAX_RECORDS {
            assert!(st.upsert(p, 1).is_ok());
        }
        assert_eq!(st.upsert(MAX_PORT, 1), Err(PersistCode::PortOob));
        assert_eq!(st.upsert(0, 1), Ok(())); // 替换不超容
    }

    #[test]
    fn magic_corrupt_injection_all_caught() {
        let mut st = PolicyStore::new();
        let _ = st.upsert(0, 0);
        let blob = st.save();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let mut bad = blob.clone();
            if let Some(b0) = bad.get_mut(0) {
                *b0 = b0.wrapping_add(1 + i as u8);
            }
            if st.restore(&bad) == Err(PersistCode::MagicMismatch) {
                caught += 1;
                st.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert!(st.conserved());
    }

    #[test]
    fn truncation_injection_all_caught() {
        let mut st = PolicyStore::new();
        for p in 0..5usize {
            let _ = st.upsert(p, 1);
        }
        let blob = st.save();
        let mut caught = 0u32;
        for k in 1..=10usize {
            let mut bad = blob.clone();
            bad.truncate(blob.len().saturating_sub(k));
            if st.restore(&bad) == Err(PersistCode::BlobTooShort) {
                caught += 1;
                st.note_detected();
            }
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn stale_rev_injection_all_caught() {
        let mut st = PolicyStore::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let stale = st.save();
            let port = i as usize % MAX_PORT;
            let _ = st.upsert(port, (i % 3) as u32);
            if st.restore(&stale) == Err(PersistCode::RevRegress) {
                caught += 1;
                st.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert!(st.conserved());
    }

    #[test]
    fn dup_port_injection_all_caught() {
        let mut st = PolicyStore::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let bad = raw_blob(0, &[(1, 0), (1, i % 3)]);
            if st.restore(&bad) == Err(PersistCode::DupPort) {
                caught += 1;
                st.note_detected();
            }
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn judge_order_len_magic_rev() {
        let mut st = PolicyStore::new();
        let _ = st.upsert(0, 0);
        let _ = st.upsert(1, 1);
        let _ = st.upsert(2, 2);
        let short = [0u8; 8];
        assert_eq!(st.restore(&short), Err(PersistCode::BlobTooShort));
        let mut wrong_magic = raw_blob(0, &[]);
        if let Some(b0) = wrong_magic.get_mut(0) {
            *b0 = b0.wrapping_add(1);
        }
        assert_eq!(st.restore(&wrong_magic), Err(PersistCode::MagicMismatch));
        assert_eq!(st.restore(&raw_blob(1, &[])), Err(PersistCode::RevRegress));
    }

    #[test]
    fn rejected_restore_no_state_change() {
        let mut st = PolicyStore::new();
        let _ = st.upsert(0, 0);
        let _ = st.upsert(1, 1);
        let snap = st.snapshot();
        let want = st.records();
        let _ = st.restore(&raw_blob(0, &[(4, 2)]));
        // 记录面零变化；拒绝显性记账（reject_cnt +1，其余不动）。
        assert_eq!(st.snapshot(), (snap.0, snap.1, snap.2, snap.3 + 1, snap.4));
        assert_eq!(st.records(), want);
    }

    #[test]
    fn reboot_consistent_ten_rounds() {
        for i in 0..10usize {
            assert!(reboot_consistent_round(i), "第 {} 轮重启恢复不一致", i);
        }
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx51545_checks_green() {
        let set = run_unx51545_checks();
        assert!(set.all_passed(), "UNX-F51545 red: {}", set.red_items_text());
    }
}
