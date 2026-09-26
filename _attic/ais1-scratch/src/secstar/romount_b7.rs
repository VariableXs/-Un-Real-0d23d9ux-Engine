//! F185 只读挂载 · 批次七深化（v7）——分块复制校验、速率历史、
//! 中途满盘恢复、影子目录对账。零堆、no_std。

use crate::checks::CheckSet;

/// 分块大小（KiB——模型层计数单位）。
pub const CHUNK_KIB: u32 = 64;
/// 每块校验环容量。
pub const CHUNK_HASH_CAP: usize = 32;
/// 速率样本环容量。
pub const SPEED_CAP: usize = 8;
/// 满盘警戒线（‰——剩余低于此先暂停再求救）。
pub const DISK_FULL_WARN_PERMILLE: u32 = 50;
/// 影子对账帧长（10B）。
pub const SHADOW_FRAME_LEN: usize = 10;

/// 分块复制校验器：每块 FNV-8 指纹入环，尾块对账（漏块/坏块检出）。
#[derive(Clone, Copy)]
pub struct ChunkVerifier {
    hashes: [u8; CHUNK_HASH_CAP],
    pub n: usize,
    pub overwritten: u32,
}

impl ChunkVerifier {
    pub const fn new() -> ChunkVerifier {
        ChunkVerifier { hashes: [0; CHUNK_HASH_CAP], n: 0, overwritten: 0 }
    }

    /// 记一块指纹（数据指纹 = FNV-1a 8bit 截断）。
    pub fn record_chunk(&mut self, data: &[u8]) {
        let mut h: u8 = 0x9B;
        for &b in data {
            h = h.wrapping_mul(0x6B).wrapping_add(b);
        }
        if self.n < CHUNK_HASH_CAP {
            self.hashes[self.n] = h;
            self.n += 1;
        } else {
            for i in 1..CHUNK_HASH_CAP {
                self.hashes[i - 1] = self.hashes[i];
            }
            self.hashes[CHUNK_HASH_CAP - 1] = h;
            self.overwritten += 1;
        }
    }

    /// 复制完整性：同数据重放指纹环必须逐位一致（读后验）。
    pub fn verify_replay(&self, replay: &ChunkVerifier) -> bool {
        if self.n != replay.n {
            return false;
        }
        for i in 0..self.n {
            if self.hashes[i] != replay.hashes[i] {
                return false;
            }
        }
        true
    }

    /// 单块重放校验：偏移 k 处指纹一致。
    pub fn chunk_matches(&self, k: usize, data: &[u8]) -> bool {
        if k >= self.n {
            return false;
        }
        let mut h: u8 = 0x9B;
        for &b in data {
            h = h.wrapping_mul(0x6B).wrapping_add(b);
        }
        self.hashes[k] == h
    }

    /// 已校验字节（n 块 × CHUNK_KIB——对账面）。
    pub fn verified_kib(&self) -> u64 {
        self.n as u64 * CHUNK_KIB as u64
    }
}

/// 速率历史：样本环 + 均值 + 趋势（限速决策的输入面）。
#[derive(Clone, Copy)]
pub struct SpeedHistory {
    kib_per_s: [u32; SPEED_CAP],
    head: usize,
    pub n: usize,
}

impl SpeedHistory {
    pub const fn new() -> SpeedHistory {
        SpeedHistory { kib_per_s: [0; SPEED_CAP], head: 0, n: 0 }
    }

    pub fn sample(&mut self, kib_per_s: u32) {
        self.kib_per_s[self.head] = kib_per_s;
        self.head = (self.head + 1) % SPEED_CAP;
        if self.n < SPEED_CAP {
            self.n += 1;
        }
    }

    pub fn mean(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        let mut sum = 0u64;
        for &v in self.kib_per_s.iter().take(self.n) {
            sum += v as u64;
        }
        Some((sum / self.n as u64) as u32)
    }

    /// 减速建议：均速 < 20MiB/s → 限速窗让路交互（大拷贝不卡前台）。
    pub fn suggest_throttle(&self) -> Option<bool> {
        Some(self.mean()? < 20 * 1_024)
    }
}

/// 中途满盘恢复：状态机 Running → Paused(满盘警戒) → 用户腾地 → Resumed / Aborted。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FullPhase {
    Running,
    Paused,
    Resumed,
    Aborted,
    Done,
}

#[derive(Clone, Copy)]
pub struct FullGuard {
    pub phase: FullPhase,
    pub pauses: u32,
    /// 暂停时的进度 ‰（恢复点——恢复后从这继续不重传）。
    pub resume_at_permille: u32,
}

impl FullGuard {
    pub const fn new() -> FullGuard {
        FullGuard { phase: FullPhase::Running, pauses: 0, resume_at_permille: 0 }
    }

    /// 空间采样：低于警戒线 → 暂停（Running 才能进 Paused）。
    pub fn on_space(&mut self, free_permille: u32, progress_permille: u32) -> bool {
        if self.phase != FullPhase::Running {
            return false;
        }
        if free_permille < DISK_FULL_WARN_PERMILLE {
            self.phase = FullPhase::Paused;
            self.pauses += 1;
            self.resume_at_permille = progress_permille;
            true
        } else {
            false
        }
    }

    /// 用户腾地后恢复：Paused → Resumed（恢复点不变——续传语义）。
    pub fn resume(&mut self) -> bool {
        if self.phase != FullPhase::Paused {
            return false;
        }
        self.phase = FullPhase::Resumed;
        true
    }

    /// 恢复后再满盘 → 再暂停（反复满盘反复停——不丢进度）。
    pub fn on_space_again(&mut self, free_permille: u32, progress_permille: u32) -> bool {
        if self.phase != FullPhase::Resumed {
            return false;
        }
        if free_permille < DISK_FULL_WARN_PERMILLE {
            self.phase = FullPhase::Paused;
            self.pauses += 1;
            self.resume_at_permille = progress_permille;
            true
        } else {
            false
        }
    }

    /// 用户放弃：Paused/Resumed → Aborted（暂停中也有出路——无死胡同）。
    pub fn abort(&mut self) -> bool {
        if self.phase == FullPhase::Done || self.phase == FullPhase::Aborted {
            return false;
        }
        self.phase = FullPhase::Aborted;
        true
    }

    /// 完成：Resumed 跑到底 → Done（Running 也可直达 Done）。
    pub fn finish(&mut self) -> bool {
        if self.phase == FullPhase::Paused || self.phase == FullPhase::Aborted {
            return false; // 暂停/放弃态不能直接宣告完成
        }
        self.phase = FullPhase::Done;
        true
    }
}

/// 影子目录对账帧（10B）：
/// [0..2) "SD" · [2..4) 源文件数 LE · [4..6) 影子文件数 LE ·
/// [6..8) 字节差 KiB LE · [8..10) 校验和（前 8B FNV-16）。
pub fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    (h & 0xFFFF) as u16
}

pub fn encode_shadow(src_files: u16, shadow_files: u16, delta_kib: u16, out: &mut [u8; SHADOW_FRAME_LEN]) -> bool {
    out[0] = b'S';
    out[1] = b'D';
    out[2..4].copy_from_slice(&src_files.to_le_bytes());
    out[4..6].copy_from_slice(&shadow_files.to_le_bytes());
    out[6..8].copy_from_slice(&delta_kib.to_le_bytes());
    let c = fnv16(&out[..8]);
    out[8] = (c & 0xFF) as u8;
    out[9] = (c >> 8) as u8;
    true
}

pub fn decode_shadow(frame: &[u8; SHADOW_FRAME_LEN]) -> Option<(u16, u16, u16)> {
    if frame[0] != b'S' || frame[1] != b'D' {
        return None;
    }
    let want = (frame[9] as u16) << 8 | frame[8] as u16;
    if fnv16(&frame[..8]) != want {
        return None;
    }
    Some((
        u16::from_le_bytes(frame[2..4].try_into().ok()?),
        u16::from_le_bytes(frame[4..6].try_into().ok()?),
        u16::from_le_bytes(frame[6..8].try_into().ok()?),
    ))
}

/// 对账结论：源/影子数相等且字节差 0 → 一致（否则给差值——差多少说多少）。
pub fn shadow_verdict(src_files: u16, shadow_files: u16, delta_kib: u16) -> (bool, u16) {
    (src_files == shadow_files && delta_kib == 0, src_files.abs_diff(shadow_files))
}

#[inline(never)]
pub fn run_romount_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F185-b7");

    // 1) 分块指纹：同数据入环后重放一致（复制完整性的总判据）。
    let mut a = ChunkVerifier::new();
    let mut b = ChunkVerifier::new();
    for i in 0..5u32 {
        let data = [i as u8; 8];
        a.record_chunk(&data);
        b.record_chunk(&data);
    }
    cs.add("chunk_replay_identical", a.verify_replay(&b) && a.n == 5, "");

    // 2) 坏块检出：第 3 块数据不同 → 重放不一致 + 单块定位命中。
    let mut c = ChunkVerifier::new();
    let mut d = ChunkVerifier::new();
    for i in 0..5u32 {
        let data = [i as u8; 8];
        c.record_chunk(&data);
        let patched = if i == 3 { [0xFF; 8] } else { [i as u8; 8] };
        d.record_chunk(&patched);
    }
    cs.add(
        "chunk_bad_block_localized",
        !c.verify_replay(&d) && c.chunk_matches(0, &[0u8; 8]) && !d.chunk_matches(3, &[3u8; 8]) && c.chunk_matches(3, &[3u8; 8]),
        "",
    );

    // 3) 环回卷：35 块 > 32 → 最旧出账 + overwritten 计数（诚实失忆）。
    let mut e = ChunkVerifier::new();
    for i in 0..35u32 {
        e.record_chunk(&[i as u8; 4]);
    }
    cs.add("chunk_ring_wraps", e.n == CHUNK_HASH_CAP && e.overwritten == 3, "");

    // 4) 校验字节账：5 块 × 64KiB = 320KiB（模型层算术直核）。
    cs.add("chunk_verified_kib", a.verified_kib() == 320, "");

    // 5) 速率均值：样本 1024/2048/3072 → 均 2048 KiB/s；空账 None。
    let mut s = SpeedHistory::new();
    s.sample(1_024);
    s.sample(2_048);
    s.sample(3_072);
    cs.add(
        "speed_mean",
        s.mean() == Some(2_048) && SpeedHistory::new().mean().is_none(),
        "",
    );

    // 6) 限速建议：慢速 <20MiB/s → 让路；快速 → 不让；空账 → None（不猜）。
    let mut s2 = SpeedHistory::new();
    s2.sample(10 * 1_024);
    let mut s3 = SpeedHistory::new();
    s3.sample(40 * 1_024);
    cs.add(
        "speed_throttle_advice",
        s2.suggest_throttle() == Some(true) && s3.suggest_throttle() == Some(false) && SpeedHistory::new().suggest_throttle().is_none(),
        "",
    );

    // 7) 满盘暂停：警戒线 49‰ 触发（50 不触发——恰点语义）并记恢复点。
    let mut g = FullGuard::new();
    let near = g.on_space(DISK_FULL_WARN_PERMILLE, 400);
    let hit = g.on_space(DISK_FULL_WARN_PERMILLE - 1, 400);
    cs.add(
        "full_pause_at_line",
        !near && hit && g.phase == FullPhase::Paused && g.resume_at_permille == 400 && g.pauses == 1,
        "",
    );

    // 8) 暂停态守门：Paused 不能 finish（没跑完不许宣告完成）、能 abort（有出路）。
    let mut g2 = FullGuard::new();
    g2.on_space(10, 100);
    let cant_finish = !g2.finish();
    let can_abort = g2.abort();
    cs.add("full_paused_guards", cant_finish && can_abort && g2.phase == FullPhase::Aborted, "");

    // 9) 恢复续传：Paused → Resumed；恢复点保持（不重传已完成部分）。
    let mut g3 = FullGuard::new();
    g3.on_space(10, 700);
    g3.resume();
    cs.add(
        "full_resume_keeps_point",
        g3.phase == FullPhase::Resumed && g3.resume_at_permille == 700,
        "",
    );

    // 10) 恢复后再满盘 → 二次暂停（反复满盘反复停，次数可账）。
    g3.on_space_again(5, 800);
    cs.add("full_re_pause", g3.phase == FullPhase::Paused && g3.pauses == 2 && g3.resume_at_permille == 800, "");

    // 11) 影子对账帧 round-trip + 撕裂拒。
    let mut f = [0u8; SHADOW_FRAME_LEN];
    assert!(encode_shadow(100, 99, 512, &mut f));
    let torn_ok = {
        let mut all = true;
        for i in 0..SHADOW_FRAME_LEN {
            let mut t = f;
            t[i] ^= 0x33;
            if decode_shadow(&t).is_some() {
                all = false;
            }
        }
        all
    };
    cs.add(
        "shadow_frame_roundtrip_tear",
        decode_shadow(&f) == Some((100, 99, 512)) && torn_ok,
        "",
    );

    // 12) 对账结论：数等差零 = 一致；差值诚实给出（差多少说多少）。
    let (ok1, d1) = shadow_verdict(100, 100, 0);
    let (ok2, d2) = shadow_verdict(100, 98, 0);
    cs.add("shadow_verdict_honest", ok1 && d1 == 0 && !ok2 && d2 == 2, "");

    // 13) 常量自洽：块 64KiB、环 32、警戒 50‰。
    cs.add(
        "b7_constants",
        CHUNK_KIB == 64 && CHUNK_HASH_CAP == 32 && DISK_FULL_WARN_PERMILLE == 50,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_ring_window() {
        // 环窗均值只看最近 8 样本（旧速率出窗不再拖均值）。
        let mut s = SpeedHistory::new();
        for i in 0..12u32 {
            s.sample((i + 1) * 1_000);
        }
        // 最近 8 个：5000..12000 → 均 8500。
        assert_eq!(s.mean(), Some(8_500));
    }

    #[test]
    fn full_guard_full_lifecycle() {
        // 全生命周期：Running→Paused→Resumed→Paused→Resumed→Done。
        let mut g = FullGuard::new();
        assert!(g.on_space(10, 100));
        assert!(g.resume());
        assert!(g.on_space_again(10, 200));
        assert!(g.resume());
        assert!(g.finish());
        assert_eq!(g.phase, FullPhase::Done);
        assert!(!g.abort(), "Done 后 abort 拒（终态不可逆）");
    }

    #[test]
    fn chunk_single_block_corruption_isolated() {
        // 中间块坏：前后块指纹不受影响（坏块定位不误伤）。
        let mut v = ChunkVerifier::new();
        for i in 0..4u32 {
            v.record_chunk(&[i as u8; 8]);
        }
        assert!(v.chunk_matches(1, &[1u8; 8]));
        assert!(!v.chunk_matches(1, &[9u8; 8]));
        assert!(v.chunk_matches(2, &[2u8; 8]));
    }
}
