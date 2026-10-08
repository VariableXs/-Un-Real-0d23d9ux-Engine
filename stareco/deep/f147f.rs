//! 深化层三 · F147 跨设备主题同步（2026-09-26 深化批次三）。
//!
//! 补深同步内核工程面（主册 G-D-22）：字段级三方合并器（base/ours/
//! theirs 冲突裁决）、版本向量比较（前/后/相等/分叉四态）、传输分块
//! 校验（逐块指纹链 + 首坏块定位）、传输进度模型（确认块推进）。

use crate::checks::CheckSet;
use crate::stareco::ebase;

// ---------------------------------------------------------------------------
// 字段级三方合并：同字段三方值 → 合并值或冲突清单
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Merge3<T: Copy + PartialEq> {
    /// 无冲突：某方改动或双方同改。
    Merged(T),
    /// 冲突：三方互异（ours≠base 且 theirs≠base 且 ours≠theirs）。
    Conflict(T, T),
}

/// 单字段三方合并：双方同改同值 → 幂等合一；一方改 → 取改方；
/// 双方改不同值 → 冲突（诚实上报，不自动裁决）。
pub fn merge_field<T: Copy + PartialEq>(base: T, ours: T, theirs: T) -> Merge3<T> {
    if ours == theirs {
        return Merge3::Merged(ours);
    }
    if ours == base {
        return Merge3::Merged(theirs);
    }
    if theirs == base {
        return Merge3::Merged(ours);
    }
    Merge3::Conflict(ours, theirs)
}

// ---------------------------------------------------------------------------
// 版本向量：(设备, 计数) 对 → 四态比较
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VvRelation {
    AHead,
    BHead,
    Equal,
    Diverged,
}

/// a 与 b 逐设备比较：全 ≥ 且 > → ahead；对称 → behind；
/// 全等 → equal；互有大小 → diverged。
pub fn vv_compare(a: &[(u8, u32)], b: &[(u8, u32)]) -> VvRelation {
    let get = |v: &[(u8, u32)], dev: u8| -> u32 {
        v.iter().find(|(d, _)| *d == dev).map(|(_, c)| *c).unwrap_or(0)
    };
    let mut devs: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    for (d, _) in a.iter().chain(b.iter()) {
        if !devs.contains(d) {
            devs.push(*d);
        }
    }
    let (mut a_gt, mut b_gt) = (false, false);
    for d in devs {
        let (ca, cb) = (get(a, d), get(b, d));
        if ca > cb {
            a_gt = true;
        }
        if cb > ca {
            b_gt = true;
        }
    }
    match (a_gt, b_gt) {
        (true, false) => VvRelation::AHead,
        (false, true) => VvRelation::BHead,
        (false, false) => VvRelation::Equal,
        (true, true) => VvRelation::Diverged,
    }
}

// ---------------------------------------------------------------------------
// 传输分块校验：块指纹链（链式 FNV）+ 首坏块定位
// ---------------------------------------------------------------------------

const CHUNK_GENESIS: u64 = 0x9E37_79B9_7F4A_7C15;

/// 生成块链：running[i] = 第 i 块后的链值（传输清单随行携带）。
pub fn build_chunk_chain(data_fps: &[u64]) -> alloc::vec::Vec<u64> {
    let mut chain = CHUNK_GENESIS;
    let mut out: alloc::vec::Vec<u64> = alloc::vec::Vec::with_capacity(data_fps.len());
    for fp in data_fps {
        chain = ebase::fnv1a64(&chain.to_be_bytes()) ^ fp;
        out.push(chain);
    }
    out
}

/// 校验：接收方以本地段指纹重放，与随行链逐块对照——首个偏离处即
/// 首坏块（Err(块序号)）。长度不一致同样定位（清单矛盾诚实上报）。
pub fn verify_chunks(local_fps: &[u64], manifest_chain: &[u64]) -> Result<(), usize> {
    if local_fps.len() != manifest_chain.len() {
        return Err(local_fps.len().min(manifest_chain.len()));
    }
    let mut chain = CHUNK_GENESIS;
    for (i, fp) in local_fps.iter().enumerate() {
        chain = ebase::fnv1a64(&chain.to_be_bytes()) ^ fp;
        if chain != manifest_chain[i] {
            return Err(i);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 传输进度模型：64KiB 块 → 总块数/末块尺寸；确认块推进百分比
// ---------------------------------------------------------------------------

pub const CHUNK_BYTES: u32 = 64 * 1024;

pub struct TransferPlan {
    pub total_bytes: u32,
}

impl TransferPlan {
    pub fn chunk_count(&self) -> u32 {
        (self.total_bytes + CHUNK_BYTES - 1) / CHUNK_BYTES
    }

    pub fn last_chunk_bytes(&self) -> u32 {
        let r = self.total_bytes % CHUNK_BYTES;
        if r == 0 {
            CHUNK_BYTES
        } else {
            r
        }
    }

    /// 确认块数 → 千分比进度（ebase 口径）。
    pub fn progress_per_mille(&self, acked_chunks: u32) -> Result<u32, &'static str> {
        if acked_chunks > self.chunk_count() {
            return Err("确认块数超总块数：进度矛盾");
        }
        Ok(acked_chunks * 1000 / self.chunk_count())
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F147F_TAG: &str = "stareco-F147-deep3";

pub fn run_f147_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F147F_TAG);

    // 三方合并
    set.add(
        "f147f merge theirs",
        merge_field(1u8, 1, 2) == Merge3::Merged(2),
        "我方未动取对方",
    );
    set.add(
        "f147f merge ours",
        merge_field(1u8, 3, 1) == Merge3::Merged(3),
        "对方未动取我方",
    );
    set.add(
        "f147f merge idempotent",
        merge_field(1u8, 4, 4) == Merge3::Merged(4),
        "双方同改合一",
    );
    set.add(
        "f147f merge conflict",
        merge_field(1u8, 4, 5) == Merge3::Conflict(4, 5),
        "互异诚实冲突",
    );

    // 版本向量
    let a = [(1u8, 3u32), (2, 1)];
    let b1 = [(1u8, 2u32), (2, 1)];
    let b2 = [(1u8, 3u32), (2, 1)];
    let b3 = [(1u8, 2u32), (2, 2)];
    let b4 = [(1u8, 4u32), (2, 1)];
    set.add("f147f vv ahead", vv_compare(&a, &b1) == VvRelation::AHead, "a 领先");
    set.add("f147f vv equal", vv_compare(&a, &b2) == VvRelation::Equal, "相等");
    set.add("f147f vv diverged", vv_compare(&a, &b3) == VvRelation::Diverged, "分叉");
    set.add("f147f vv behind", vv_compare(&a, &b4) == VvRelation::BHead, "a 落后");
    set.add(
        "f147f vv missing dev",
        vv_compare(&a, &[(1u8, 3u32)]) == VvRelation::AHead,
        "缺设备按 0 计",
    );

    // 分块校验（随行链逐块对照 → 首坏块可定位）
    let c1 = 0xAAAAu64;
    let c2 = 0xBBBBu64;
    let c3 = 0xCCCCu64;
    let good = build_chunk_chain(&[c1, c2, c3]);
    set.add("f147f chunks ok", verify_chunks(&[c1, c2, c3], &good).is_ok(), "链完整");
    set.add(
        "f147f chunks localize",
        verify_chunks(&[c1, 0xDEAD, c3], &good) == Err(1),
        "篡改第二块定位到 1",
    );
    set.add(
        "f147f chunks len mismatch",
        verify_chunks(&[c1, c2], &good) == Err(2),
        "块数矛盾诚实上报",
    );
    set.add("f147f chunks empty ok", verify_chunks(&[], &[]).is_ok(), "空块空链自洽");

    // 传输进度
    let plan = TransferPlan { total_bytes: 64 * 1024 * 2 + 100 };
    set.add("f147f chunks 3", plan.chunk_count() == 3, "两整块+尾块");
    set.add("f147f last chunk", plan.last_chunk_bytes() == 100, "尾块 100 字节");
    set.add("f147f progress", plan.progress_per_mille(1) == Ok(333), "1/3=333‰");
    set.add("f147f progress over", plan.progress_per_mille(4).is_err(), "确认超总拒");
    let whole = TransferPlan { total_bytes: CHUNK_BYTES };
    set.add("f147f exact chunk", whole.chunk_count() == 1 && whole.last_chunk_bytes() == CHUNK_BYTES, "整块恰好");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn chunk_chain_roundtrip() {
        let fps = [1u64, 2, 3, 4, 5];
        let manifest = build_chunk_chain(&fps);
        assert_eq!(manifest.len(), 5);
        assert!(verify_chunks(&fps, &manifest).is_ok());
        // 第三块篡改定位到 2。
        let mut bad = fps;
        bad[2] = 999;
        assert_eq!(verify_chunks(&bad, &manifest), Err(2));
        // 尾块篡改定位到最后。
        let mut bad2 = fps;
        bad2[4] = 0;
        assert_eq!(verify_chunks(&bad2, &manifest), Err(4));
    }

    #[test]
    fn merge_conflict_symmetric() {
        let m = merge_field("a", "b", "c");
        assert_eq!(m, Merge3::Conflict("b", "c"));
        // 冲突不可逆推——双方值都保留（合并器不裁决审美）。
        match m {
            Merge3::Conflict(o, t) => {
                assert_eq!(o, "b");
                assert_eq!(t, "c");
            }
            _ => panic!("expect conflict"),
        }
    }
}
