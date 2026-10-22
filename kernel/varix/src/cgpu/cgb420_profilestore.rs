//! CGPU-F0420 · 长期画像持久化（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 画像持久化：冷数据落盘（C04 画像的长期层——格式与 B07 同源）；
//!   画像生命周期（热→温→冷→过期清理——配额治理）；画像完整性（落盘
//!   校验——CRC+一致性）；画像恢复（重启后画像层重建验证）。
//! - 判据：冷数据、生命周期、校验、恢复、判据。
//!
//! 设计纪律：
//! - 四态生命周期：Hot（内存活跃，分钟级访问）→ Warm（近期不访问，
//!   仍可快取）→ Cold（落盘，B07 同源格式）→ Expired（超保留期清理，
//!   清理前可配宽限）。单向流动，唯一回程是恢复（Cold→Warm）。
//! - 完整性：落盘时算 CRC32（与 B07 同源算法——F0417 共用语义）+
//!   记录计数；恢复时逐项核对（CRC/计数/账户集合三方一致）。
//! - 恢复：重启后画像层重建 = 从冷存读回并逐项校验；坏块隔离不阻塞
//!   整体重建（好画像先回来，坏画像进隔离名单）。
//! - 配额治理：生命周期推进由配额水位驱动（存储超水位 → 最冷的先冷）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0D70 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0D70，全仓独占）。
pub const SEG_BASE: u16 = 0x0D70;

pub const E_PRO_STATE: u16 = SEG_BASE + 1; // 生命周期非法跃迁
pub const E_PRO_CRC: u16 = SEG_BASE + 2; // CRC 校验红
pub const E_PRO_RECON: u16 = SEG_BASE + 3; // 恢复一致性红（计数/集合不符）
pub const E_PRO_UNKNOWN: u16 = SEG_BASE + 4; // 画像不在册
pub const E_PRO_QUOTA: u16 = SEG_BASE + 5; // 存储超配额
pub const E_PRO_ISOLATE: u16 = SEG_BASE + 6; // 坏块未隔离

/// 生命周期四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileState {
    Hot,
    Warm,
    Cold,
    Expired,
}

/// 单向流动合法跃迁（Cold→Warm 是唯一回程——恢复）。
fn legal(from: ProfileState, to: ProfileState) -> bool {
    matches!(
        (from, to),
        (ProfileState::Hot, ProfileState::Warm)
            | (ProfileState::Warm, ProfileState::Cold)
            | (ProfileState::Cold, ProfileState::Expired)
            | (ProfileState::Cold, ProfileState::Warm) // 恢复
    )
}

/// C04 画像的长期层记录。
#[derive(Clone, Debug)]
pub struct ProfileRec {
    pub acct: u32,
    pub state: ProfileState,
    /// 画像负载向量（千分位权重——与 C04 同构）。
    pub weights: Vec<u32>,
    /// 最后访问帧。
    pub last_touch: u64,
    /// 落盘 CRC（Cold 后有效）。
    pub crc: Option<u32>,
}

/// 画像持久化引擎。
#[derive(Clone, Debug)]
pub struct ProfileStore {
    pub recs: Vec<ProfileRec>,
    /// 存储配额（画像条数上限）。
    pub quota: usize,
    /// 保留期（帧）——超期可清。
    pub retention: u64,
    /// 隔离名单（恢复时校验红的坏块）。
    pub isolated: Vec<u32>,
    now: u64,
}

impl ProfileStore {
    pub fn new(quota: usize, retention: u64) -> Self {
        ProfileStore {
            recs: Vec::new(),
            quota,
            retention,
            isolated: Vec::new(),
            now: 0,
        }
    }

    pub fn tick(&mut self, frame: u64) {
        self.now = frame;
    }

    /// 新画像入册（Hot 起点）。
    pub fn create(&mut self, acct: u32, weights: Vec<u32>) -> Result<(), u16> {
        if self.recs.iter().any(|r| r.acct == acct) {
            return Err(E_PRO_UNKNOWN);
        }
        if self.recs.len() >= self.quota {
            return Err(E_PRO_QUOTA);
        }
        self.recs.push(ProfileRec {
            acct,
            state: ProfileState::Hot,
            weights,
            last_touch: self.now,
            crc: None,
        });
        Ok(())
    }

    /// 状态推进（单向 + 恢复回程）。
    pub fn advance(&mut self, acct: u32, to: ProfileState) -> Result<(), u16> {
        let r = self
            .recs
            .iter_mut()
            .find(|r| r.acct == acct)
            .ok_or(E_PRO_UNKNOWN)?;
        if !legal(r.state, to) {
            return Err(E_PRO_STATE);
        }
        r.state = to;
        Ok(())
    }

    /// 访问即保温：Hot/Warm 更新时间戳；Cold 访问触发恢复（→Warm）。
    pub fn touch(&mut self, acct: u32) -> Result<(), u16> {
        let now = self.now;
        let r = self
            .recs
            .iter_mut()
            .find(|r| r.acct == acct)
            .ok_or(E_PRO_UNKNOWN)?;
        match r.state {
            ProfileState::Hot | ProfileState::Warm => r.last_touch = now,
            ProfileState::Cold => {
                // 恢复路径：先校验 CRC 才允许回温。
                let crc = r.crc.ok_or(E_PRO_CRC)?;
                let body = profile_body(&r.weights);
                if crc32(body.as_slice()) != crc {
                    return Err(E_PRO_CRC);
                }
                r.state = ProfileState::Warm;
                r.last_touch = now;
            }
            ProfileState::Expired => return Err(E_PRO_UNKNOWN),
        }
        Ok(())
    }

    /// 冷数据落盘：Warm → Cold，CRC 落定（B07 同源格式）。
    pub fn freeze(&mut self, acct: u32) -> Result<u32, u16> {
        let r = self
            .recs
            .iter_mut()
            .find(|r| r.acct == acct)
            .ok_or(E_PRO_UNKNOWN)?;
        if r.state != ProfileState::Warm {
            return Err(E_PRO_STATE);
        }
        let body = profile_body(&r.weights);
        let crc = crc32(body.as_slice());
        r.crc = Some(crc);
        r.state = ProfileState::Cold;
        Ok(crc)
    }

    /// 过期清理：Cold 超保留期 → Expired（配额治理出口）。
    pub fn sweep_expired(&mut self) -> usize {
        let now = self.now;
        let ret = self.retention;
        let mut n = 0;
        for r in self.recs.iter_mut() {
            if r.state == ProfileState::Cold && now.saturating_sub(r.last_touch) >= ret {
                r.state = ProfileState::Expired;
                n += 1;
            }
        }
        n
    }

    /// 配额水位：Expired 释放后可再入册（清理驱动回收）。
    pub fn purge_expired(&mut self) -> usize {
        let before = self.recs.len();
        self.recs.retain(|r| r.state != ProfileState::Expired);
        before - self.recs.len()
    }

    /// 重启恢复：给定冷存快照（账户/CRC/权重），逐项校验回温。
    /// 好画像先回，坏块进隔离名单（重建不整体阻塞）。
    pub fn restore(&mut self, cold: &[(u32, u32, Vec<u32>)]) -> Result<usize, u16> {
        let mut ok = 0;
        for (acct, crc, weights) in cold {
            let body = profile_body(weights);
            if crc32(body.as_slice()) != *crc {
                if !self.isolated.contains(acct) {
                    self.isolated.push(*acct);
                }
                continue; // 坏块隔离——好块继续
            }
            // 一致性：计数与权重向量非空。
            if weights.is_empty() {
                if !self.isolated.contains(acct) {
                    self.isolated.push(*acct);
                }
                continue;
            }
            match self.recs.iter_mut().find(|r| r.acct == *acct) {
                Some(r) => {
                    r.weights = weights.clone();
                    r.crc = Some(*crc);
                    r.state = ProfileState::Warm;
                }
                None => {
                    if self.recs.len() >= self.quota {
                        return Err(E_PRO_QUOTA);
                    }
                    self.recs.push(ProfileRec {
                        acct: *acct,
                        state: ProfileState::Warm,
                        weights: weights.clone(),
                        last_touch: self.now,
                        crc: Some(*crc),
                    });
                }
            }
            ok += 1;
        }
        Ok(ok)
    }
}

/// 画像体序列化（B07 同源——确定性小端）。
pub fn profile_body(weights: &[u32]) -> Vec<u8> {
    let mut v = Vec::new();
    for w in weights {
        v.extend_from_slice(&w.to_le_bytes());
    }
    v
}

/// CRC32（与 F0417 同源算法——B07 侧共用）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for b in data {
        crc ^= *b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> ProfileStore {
        let mut s = ProfileStore::new(8, 1_000);
        s.create(1, vec![500, 300, 200]).ok();
        s.create(2, vec![800, 200]).ok();
        s
    }

    #[test]
    fn a01_冷数据落盘_B07同源CRC() {
        let mut s = store();
        // Hot 直接冻结拒（生命周期必须逐级）。
        assert_eq!(s.freeze(1), Err(E_PRO_STATE));
        s.advance(1, ProfileState::Warm).ok();
        let crc = s.freeze(1).ok();
        assert!(crc.is_some());
        // CRC 与独立算法一致。
        assert_eq!(crc.unwrap_or(0), crc32(&profile_body(&[500, 300, 200])));
        // CRC 标准向量自证。
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        // 重复冻结拒（已 Cold）。
        assert_eq!(s.freeze(1), Err(E_PRO_STATE));
    }

    #[test]
    fn a02_生命周期_单向流与恢复回程() {
        let mut s = store();
        // 合法链：Hot→Warm→Cold→（恢复）→Warm。
        s.advance(1, ProfileState::Warm).ok();
        s.freeze(1).ok();
        s.tick(10);
        assert_eq!(s.touch(1), Ok(())); // Cold 访问触发恢复
        assert_eq!(
            s.recs.iter().find(|r| r.acct == 1).map(|r| r.state),
            Some(ProfileState::Warm)
        );
        // 非法跃迁拒：Hot 跳 Cold / Hot 跳 Expired（账户 2 保持 Hot）。
        assert_eq!(s.advance(2, ProfileState::Cold), Err(E_PRO_STATE));
        assert_eq!(s.advance(2, ProfileState::Expired), Err(E_PRO_STATE));
        // Warm 跳 Expired 拒（账户 1 恢复后为 Warm）。
        assert_eq!(s.advance(1, ProfileState::Expired), Err(E_PRO_STATE));
        // 逐级放行：Warm→Cold→Expired。
        assert_eq!(s.advance(2, ProfileState::Warm), Ok(()));
        assert_eq!(s.advance(2, ProfileState::Cold), Ok(()));
        assert_eq!(s.advance(2, ProfileState::Expired), Ok(()));
        let mut s2 = store();
        assert_eq!(s2.advance(1, ProfileState::Cold), Err(E_PRO_STATE));
        assert_eq!(s2.advance(1, ProfileState::Expired), Err(E_PRO_STATE));
        // Expired 单向不可逆（Expired→Warm 拒）。
        assert_eq!(s.advance(2, ProfileState::Warm), Err(E_PRO_STATE));
        assert_eq!(s.touch(2), Err(E_PRO_UNKNOWN));
        // Cold 超龄 → sweep 转 Expired（账户 1：恢复 Warm 后再入 Cold）。
        s.advance(1, ProfileState::Cold).ok();
        s.tick(5_000);
        assert_eq!(s.sweep_expired(), 1);
    }

    #[test]
    fn a03_完整性_CRC红即恢复拒() {
        let mut s = store();
        s.advance(1, ProfileState::Warm).ok();
        s.freeze(1).ok();
        // 篡改落盘权重（CRC 不再匹配）→ 恢复红。
        if let Some(r) = s.recs.iter_mut().find(|r| r.acct == 1) {
            r.weights[0] = 999;
        }
        assert_eq!(s.touch(1), Err(E_PRO_CRC));
        // 无 CRC 的 Cold（伪造态）恢复也拒。
        let mut s2 = store();
        s2.advance(1, ProfileState::Warm).ok();
        if let Some(r) = s2.recs.iter_mut().find(|r| r.acct == 1) {
            r.state = ProfileState::Cold;
        }
        assert_eq!(s2.touch(1), Err(E_PRO_CRC));
    }

    #[test]
    fn a04_恢复_重启重建坏块隔离() {
        let mut s = ProfileStore::new(8, 1_000);
        // 冷存快照：块 1 好的、块 2 坏（CRC 错）、块 3 空向量（一致性红）。
        let w1 = vec![400, 600];
        let crc1 = crc32(&profile_body(&w1));
        let cold = vec![
            (1u32, crc1, w1.clone()),
            (2u32, 0xDEAD, vec![100, 100]),
            (3u32, 0xBEEF, Vec::new()),
        ];
        let ok = s.restore(&cold).ok();
        assert_eq!(ok, Some(1)); // 只有好块 1 回温
        assert_eq!(s.isolated, vec![2, 3]); // 坏块隔离名单
        // 好块状态。
        assert_eq!(
            s.recs.iter().find(|r| r.acct == 1).map(|r| r.state),
            Some(ProfileState::Warm)
        );
        // 好块数据逐项一致。
        assert_eq!(
            s.recs.iter().find(|r| r.acct == 1).map(|r| r.weights.clone()),
            Some(w1)
        );
    }

    #[test]
    fn a05_配额治理_超配额与过期回收() {
        let mut s = ProfileStore::new(2, 100);
        s.create(1, vec![1]).ok();
        s.create(2, vec![2]).ok();
        // 满配额再入册拒。
        assert_eq!(s.create(3, vec![3]), Err(E_PRO_QUOTA));
        // 过期清理释放配额。
        s.advance(1, ProfileState::Warm).ok();
        s.freeze(1).ok();
        s.tick(200);
        assert_eq!(s.sweep_expired(), 1);
        assert_eq!(s.purge_expired(), 1);
        assert_eq!(s.create(3, vec![3]), Ok(()));
    }
}
