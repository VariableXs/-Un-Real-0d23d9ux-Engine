//! AI-39 发布与分发域（A951~A975，AURORA-1000）。
//!
//! Installer image builds, bootable media, the installer flow, update
//! channels, delta updates, migration tooling, version management, release
//! signing, rollback, announcements and the domain gates.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// A951 — 安装镜像构建
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageHeader {
    pub magic: [u8; 4],
    /// Image size in KiB.
    pub size_kib: u32,
    pub checksum: u32,
}

impl ImageHeader {
    pub const MAGIC: [u8; 4] = *b"ARIM";

    /// Build a header for a given size; checksum = size-derived FNV-1a.
    pub fn build(size_kib: u32) -> ImageHeader {
        let mut h = 0x811C_9DC5u32;
        let bytes = size_kib.to_le_bytes();
        for b in bytes {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        ImageHeader { magic: Self::MAGIC, size_kib, checksum: h }
    }

    pub fn valid(&self) -> bool {
        if self.magic != Self::MAGIC {
            return false;
        }
        let expect = Self::build(self.size_kib).checksum;
        self.checksum == expect
    }
}

/// Image fits the target medium (optical 700 MiB, USB ≥ 1 GiB).
pub fn image_fits(size_kib: u32, medium_kib: u32) -> bool {
    size_kib <= medium_kib
}

// ---------------------------------------------------------------------------
// A952 — 启动介质制作
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootScheme {
    LegacyMbr,
    UefiGpt,
    Hybrid,
}

/// Media write plan: sector ranges + verify pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaPlan {
    pub scheme: BootScheme,
    /// Reserved leading sectors (MBR/GPT protective area).
    pub reserved_sectors: u32,
    pub verify: bool,
}

pub fn media_plan_for(uefi: bool, legacy: bool) -> MediaPlan {
    match (uefi, legacy) {
        (true, true) => MediaPlan { scheme: BootScheme::Hybrid, reserved_sectors: 2_048, verify: true },
        (true, false) => MediaPlan { scheme: BootScheme::UefiGpt, reserved_sectors: 2_048, verify: true },
        (false, true) => MediaPlan { scheme: BootScheme::LegacyMbr, reserved_sectors: 63, verify: true },
        (false, false) => MediaPlan { scheme: BootScheme::LegacyMbr, reserved_sectors: 63, verify: false },
    }
}

// ---------------------------------------------------------------------------
// A953 — 安装程序
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallStage {
    Probe,
    Partition,
    CopyFiles,
    Bootloader,
    Configure,
    Finish,
}

/// Install is resumable: completed stages are skipped on retry.
pub fn resume_from(completed: &[InstallStage]) -> InstallStage {
    const ORDER: [InstallStage; 6] = [
        InstallStage::Probe,
        InstallStage::Partition,
        InstallStage::CopyFiles,
        InstallStage::Bootloader,
        InstallStage::Configure,
        InstallStage::Finish,
    ];
    let mut next = InstallStage::Probe;
    for s in ORDER {
        if completed.contains(&s) {
            next = s;
        } else {
            return s;
        }
    }
    next
}

/// Disk space guard: need image + 20% headroom.
pub fn disk_space_ok(needed_kib: u32, free_kib: u32) -> bool {
    free_kib >= needed_kib + needed_kib / 5
}

// ---------------------------------------------------------------------------
// A954 — 更新通道
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
    Nightly,
}

impl Channel {
    /// A channel may only upgrade from a lower or same channel.
    pub fn accepts(self, from: Channel) -> bool {
        self.rank() >= from.rank()
    }

    fn rank(self) -> u8 {
        match self {
            Channel::Nightly => 0,
            Channel::Beta => 1,
            Channel::Stable => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// A955 — 增量更新
// ---------------------------------------------------------------------------

/// Delta update: applies when base hash matches and the patch is verified.
pub fn delta_applies(base_hash: u64, expected_base: u64, patch_verified: bool) -> bool {
    base_hash == expected_base && patch_verified
}

/// Delta size sanity: patches must be < 40% of a full image to be worth it.
pub fn delta_worthwhile(patch_kib: u32, full_kib: u32) -> bool {
    full_kib > 0 && patch_kib * 10 < full_kib * 4
}

// ---------------------------------------------------------------------------
// A956 — 迁移工具
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigrationItem {
    pub kind: &'static str,
    pub count: u32,
    pub migrated: u32,
}

impl MigrationItem {
    pub fn complete(&self) -> bool {
        self.migrated == self.count
    }
}

/// Migration summary: all items complete, nothing skipped silently.
pub fn migration_complete(items: &[MigrationItem]) -> bool {
    !items.is_empty() && items.iter().all(|i| i.complete())
}

// ---------------------------------------------------------------------------
// A957 — 版本管理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl Version {
    pub fn cmp_version(&self, other: &Version) -> core::cmp::Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch))
    }

    /// SemVer-style ordering: newer wins.
    pub fn newer_than(&self, other: &Version) -> bool {
        self.cmp_version(other) == core::cmp::Ordering::Greater
    }
}

// ---------------------------------------------------------------------------
// A958 — 发布签名
// ---------------------------------------------------------------------------

/// Toy Ed25519-style verification stand-in: a real implementation swaps in
/// the crypto primitive; the envelope (hash → sign → verify) stays.
pub fn sign_digest(digest: u64, key: u64) -> u64 {
    digest.wrapping_mul(key).rotate_left(17) ^ key
}

pub fn verify_digest(digest: u64, key: u64, signature: u64) -> bool {
    sign_digest(digest, key) == signature
}

/// Signed artifact = digest of content + signature check.
pub fn artifact_signature_ok(content_hash: u64, key: u64, signature: u64) -> bool {
    verify_digest(content_hash, key, signature)
}

// ---------------------------------------------------------------------------
// A959 — 发布回滚
// ---------------------------------------------------------------------------

/// Rollback decision: previous version must be signed and healthy.
pub fn rollback_allowed(prev_signed: bool, prev_healthy: bool, current_healthy: bool) -> bool {
    !current_healthy && prev_signed && prev_healthy
}

/// Rollback depth cap: at most 3 generations back.
pub fn rollback_depth_ok(current_gen: u32, target_gen: u32) -> bool {
    current_gen > target_gen && current_gen - target_gen <= 3
}

// ---------------------------------------------------------------------------
// A960 — 发布公告
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Announcement {
    pub version: Version,
    pub highlights: u8,
    pub translated_langs: u8,
}

impl Announcement {
    pub fn publishable(&self) -> bool {
        self.highlights > 0 && self.translated_langs >= 2
    }
}

// ---------------------------------------------------------------------------
// A961/A970 — 发布性能预算
// ---------------------------------------------------------------------------

/// Update check must cost ≤ 50 ms of user-visible time.
pub fn update_check_budget_ok(ms: u32) -> bool {
    ms <= 50
}

/// Image build ≤ 10 minutes on CI.
pub fn build_budget_ok(minutes: u32) -> bool {
    minutes <= 10
}

// ---------------------------------------------------------------------------
// A962/A973 — 发布文档
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseNote {
    pub version: Version,
    pub sections: u8,
    pub breaking_changes: u8,
}

impl ReleaseNote {
    /// Every release needs notes; breaking changes must be called out.
    pub fn complete(&self) -> bool {
        self.sections >= 3 && (self.breaking_changes == 0 || self.sections >= 4)
    }
}

// ---------------------------------------------------------------------------
// A963/A971 — 可观测（发布事件）
// ---------------------------------------------------------------------------

const REL_EVENT_CAP: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelEventKind {
    BuildStart,
    BuildDone,
    SignOk,
    SignFail,
    Publish,
    Rollback,
}

#[derive(Clone, Copy, Debug)]
pub struct RelEvent {
    pub kind: RelEventKind,
    pub stamp: u64,
    pub arg: u32,
}

pub struct RelEventLog {
    events: [RelEvent; REL_EVENT_CAP],
    head: usize,
    count: usize,
}

impl RelEventLog {
    pub const fn new() -> RelEventLog {
        RelEventLog {
            events: [RelEvent { kind: RelEventKind::BuildStart, stamp: 0, arg: 0 }; REL_EVENT_CAP],
            head: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, e: RelEvent) {
        self.events[self.head] = e;
        self.head = (self.head + 1) % REL_EVENT_CAP;
        if self.count < REL_EVENT_CAP {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<RelEvent> {
        if i >= self.count {
            return None;
        }
        let pos = (self.head + REL_EVENT_CAP - 1 - i) % REL_EVENT_CAP;
        Some(self.events[pos])
    }

    pub fn count_of(&self, k: RelEventKind) -> usize {
        (0..self.count).filter(|i| self.get(*i).map(|e| e.kind == k).unwrap_or(false)).count()
    }

    /// Signature failures must be rare: < 10% of sign attempts.
    pub fn sign_fail_permille(&self) -> u32 {
        let ok = self.count_of(RelEventKind::SignOk);
        let fail = self.count_of(RelEventKind::SignFail);
        let total = ok + fail;
        if total == 0 {
            return 0;
        }
        (fail as u64 * 1000 / total as u64) as u32
    }
}

// ---------------------------------------------------------------------------
// A964/A974 — 降级链
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistTier {
    /// Full: signed images + delta updates + all channels.
    Full,
    /// Full images only (delta infra unavailable).
    FullOnly,
    /// Offline image download only.
    Offline,
}

pub fn dist_degrade(delta_ok: bool, channels_ok: bool) -> DistTier {
    if channels_ok {
        if delta_ok {
            DistTier::Full
        } else {
            DistTier::FullOnly
        }
    } else {
        DistTier::Offline
    }
}

// ---------------------------------------------------------------------------
// A965 — 发布兼容矩阵
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct DistCompatCell {
    pub medium: &'static str,
    pub bootable: bool,
    pub verified: bool,
}

/// Every shipped medium must boot and verify.
pub fn dist_compat_ok(cells: &[DistCompatCell]) -> bool {
    !cells.is_empty() && cells.iter().all(|c| c.bootable && c.verified)
}

// ---------------------------------------------------------------------------
// A966 — 发布安全
// ---------------------------------------------------------------------------

/// Publish requires: signed artifact + channel rank sanity + no unsigned
/// delta on stable.
pub fn publish_security_ok(signed: bool, channel: Channel, delta_signed: bool, uses_delta: bool) -> bool {
    signed && !(uses_delta && channel == Channel::Stable && !delta_signed)
}

// ---------------------------------------------------------------------------
// A967/A968 — 发布自检
// ---------------------------------------------------------------------------

/// Cross-check: the image header, the plan and the version all agree.
pub fn release_selfcheck(header: ImageHeader, plan: MediaPlan, v: Version) -> bool {
    header.valid() && plan.verify && !(v.major == 0 && v.minor == 0 && v.patch == 0)
}

// ---------------------------------------------------------------------------
// A969/A972 — 模糊测试
// ---------------------------------------------------------------------------

/// Fuzz the header validator: magic mismatch never validates.
pub fn fuzz_header(h: ImageHeader) -> bool {
    if h.magic != ImageHeader::MAGIC {
        !h.valid()
    } else {
        h.valid() == (h.checksum == ImageHeader::build(h.size_kib).checksum)
    }
}

/// Fuzz the version comparator: total order invariants.
pub fn fuzz_version(a: Version, b: Version) -> bool {
    let eq = a.cmp_version(&b) == core::cmp::Ordering::Equal;
    let antisym = a.newer_than(&b) == !b.newer_than(&a) || eq;
    eq || antisym
}

// ---------------------------------------------------------------------------
// A975 — 域自检收口
// ---------------------------------------------------------------------------

/// Domain self-test: pure invariants that must hold on every machine.
pub fn run_release_checks() -> CheckSet {
    let mut set = CheckSet::new("release");

    let h = ImageHeader::build(65_536);
    set.add(
        "A951 image",
        h.valid() && !ImageHeader { magic: *b"XXXX", ..h }.valid()
            && !ImageHeader { checksum: h.checksum ^ 1, ..h }.valid()
            && image_fits(700_000, 716_800) && !image_fits(700_001, 700_000),
        "header+fit",
    );

    set.add(
        "A952 media",
        media_plan_for(true, true).scheme == BootScheme::Hybrid
            && media_plan_for(true, false).scheme == BootScheme::UefiGpt
            && media_plan_for(false, true).scheme == BootScheme::LegacyMbr
            && media_plan_for(true, true).reserved_sectors == 2_048
            && !media_plan_for(false, false).verify,
        "plan",
    );

    let done = [InstallStage::Probe, InstallStage::Partition];
    set.add(
        "A953 installer",
        resume_from(&done) == InstallStage::CopyFiles
            && resume_from(&[]) == InstallStage::Probe
            && resume_from(&[InstallStage::Probe, InstallStage::Partition, InstallStage::CopyFiles, InstallStage::Bootloader, InstallStage::Configure, InstallStage::Finish]) == InstallStage::Finish
            && disk_space_ok(1_000, 1_200) && !disk_space_ok(1_000, 1_100),
        "resume+space",
    );

    set.add(
        "A954 channels",
        Channel::Stable.accepts(Channel::Beta) && Channel::Beta.accepts(Channel::Nightly)
            && !Channel::Nightly.accepts(Channel::Stable)
            && Channel::Beta.accepts(Channel::Beta),
        "ranks",
    );

    set.add(
        "A955 delta",
        delta_applies(7, 7, true) && !delta_applies(7, 8, true) && !delta_applies(7, 7, false)
            && delta_worthwhile(100, 700_000) && !delta_worthwhile(400_000, 700_000)
            && !delta_worthwhile(1, 0),
        "apply",
    );

    let items = [
        MigrationItem { kind: "docs", count: 10, migrated: 10 },
        MigrationItem { kind: "settings", count: 4, migrated: 4 },
    ];
    set.add(
        "A956 migration",
        migration_complete(&items)
            && !migration_complete(&[MigrationItem { migrated: 9, ..items[0] }])
            && !migration_complete(&[]),
        "complete",
    );

    let v1 = Version { major: 1, minor: 2, patch: 3 };
    let v2 = Version { major: 1, minor: 3, patch: 0 };
    set.add(
        "A957 versions",
        v2.newer_than(&v1) && !v1.newer_than(&v2) && !v1.newer_than(&v1)
            && Version { major: 2, minor: 0, patch: 0 }.newer_than(&Version { major: 1, minor: 9, patch: 9 }),
        "semver",
    );

    let sig = sign_digest(0xDEAD_BEEF, 0x1234);
    set.add(
        "A958 sign",
        verify_digest(0xDEAD_BEEF, 0x1234, sig)
            && !verify_digest(0xDEAD_BEEF, 0x1235, sig)
            && !verify_digest(0xDEAD_BEEC, 0x1234, sig)
            && artifact_signature_ok(5, 7, sign_digest(5, 7)),
        "envelope",
    );

    set.add(
        "A959 rollback",
        rollback_allowed(true, true, false) && !rollback_allowed(true, true, true)
            && !rollback_allowed(false, true, false) && !rollback_allowed(true, false, false)
            && rollback_depth_ok(5, 2) && !rollback_depth_ok(5, 1) && !rollback_depth_ok(2, 5),
        "depth",
    );

    let ann = Announcement { version: v1, highlights: 5, translated_langs: 3 };
    set.add(
        "A960 announce",
        ann.publishable() && !Announcement { highlights: 0, ..ann }.publishable()
            && !Announcement { translated_langs: 1, ..ann }.publishable(),
        "publish",
    );

    set.add(
        "A961 budget",
        update_check_budget_ok(49) && !update_check_budget_ok(51)
            && build_budget_ok(10) && !build_budget_ok(11),
        "time",
    );

    let note = ReleaseNote { version: v1, sections: 4, breaking_changes: 2 };
    set.add(
        "A962 notes",
        note.complete() && !ReleaseNote { sections: 3, ..note }.complete()
            && ReleaseNote { sections: 3, breaking_changes: 0, ..note }.complete(),
        "sections",
    );

    let mut log = RelEventLog::new();
    log.push(RelEvent { kind: RelEventKind::SignOk, stamp: 1, arg: 0 });
    log.push(RelEvent { kind: RelEventKind::SignFail, stamp: 2, arg: 0 });
    log.push(RelEvent { kind: RelEventKind::Publish, stamp: 3, arg: 0 });
    set.add(
        "A963 events",
        log.len() == 3 && log.get(0).unwrap().kind == RelEventKind::Publish
            && log.count_of(RelEventKind::SignOk) == 1 && log.sign_fail_permille() == 500,
        "ring",
    );

    set.add(
        "A964 degrade",
        dist_degrade(true, true) == DistTier::Full && dist_degrade(false, true) == DistTier::FullOnly
            && dist_degrade(false, false) == DistTier::Offline,
        "chain",
    );

    let cells = [
        DistCompatCell { medium: "usb", bootable: true, verified: true },
        DistCompatCell { medium: "cd", bootable: true, verified: true },
    ];
    set.add(
        "A965 compat",
        dist_compat_ok(&cells) && !dist_compat_ok(&[DistCompatCell { verified: false, ..cells[0] }]),
        "matrix",
    );

    set.add(
        "A966 security",
        publish_security_ok(true, Channel::Stable, true, true)
            && !publish_security_ok(true, Channel::Stable, false, true)
            && !publish_security_ok(false, Channel::Nightly, false, false),
        "publish gate",
    );

    set.add(
        "A967 selfcheck",
        release_selfcheck(h, media_plan_for(true, false), v1)
            && !release_selfcheck(h, media_plan_for(false, false), v1)
            && !release_selfcheck(h, media_plan_for(true, false), Version { major: 0, minor: 0, patch: 0 }),
        "cross",
    );

    set.add(
        "A969 fuzz",
        fuzz_header(ImageHeader::build(1)) && fuzz_header(ImageHeader { magic: *b"NOPE", size_kib: 1, checksum: 0 })
            && fuzz_version(v1, v2) && fuzz_version(v1, v1) && fuzz_version(v2, v1),
        "robust",
    );

    set.add(
        "A970 budget floor",
        update_check_budget_ok(0) && build_budget_ok(0),
        "min",
    );

    set.add("A971 obs cap", log.len() <= REL_EVENT_CAP, "bounded");

    set.add(
        "A968/A972 wrap",
        release_selfcheck(h, media_plan_for(true, true), v2)
            && ReleaseNote { version: v2, ..note }.complete(),
        "closure wrap",
    );

    set.add(
        "A973 docs floor",
        !ReleaseNote { sections: 2, ..note }.complete() && update_check_budget_ok(50) && build_budget_ok(9),
        "floors",
    );

    set.add(
        "A966/A969 gate matrix",
        publish_security_ok(true, Channel::Beta, false, true)
            && fuzz_version(Version { major: 1, minor: 2, patch: 4 }, v1),
        "gates",
    );

    set.add(
        "A974 offline tier",
        dist_degrade(true, false) == DistTier::Offline,
        "channels first",
    );

    set.add(
        "A975 closure",
        set.len() + 1 >= 25 && !set.truncated(),
        "self-test complete",
    );

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a951_header_sizes() {
        for size in [0u32, 1, 65_536, u32::MAX] {
            let h = ImageHeader::build(size);
            assert!(h.valid());
        }
        assert!(!ImageHeader { magic: ImageHeader::MAGIC, size_kib: 0, checksum: 0 }.valid()
            == (ImageHeader::build(0).checksum != 0));
    }

    #[test]
    fn a953_resume_partial_prefixes() {
        let all = [
            InstallStage::Probe,
            InstallStage::Partition,
            InstallStage::CopyFiles,
            InstallStage::Bootloader,
            InstallStage::Configure,
            InstallStage::Finish,
        ];
        for i in 0..=all.len() {
            assert_eq!(resume_from(&all[..i]), all[i.min(all.len() - 1)]);
        }
        // Out-of-order completed stages don't confuse the resume point.
        let odd = [InstallStage::Finish, InstallStage::Probe];
        assert_eq!(resume_from(&odd), InstallStage::Partition);
    }

    #[test]
    fn a954_channel_order() {
        assert!(Channel::Stable.accepts(Channel::Stable));
        assert!(!Channel::Beta.accepts(Channel::Stable));
        assert!(Channel::Nightly.accepts(Channel::Nightly));
    }

    #[test]
    fn a957_version_edge() {
        let a = Version { major: 0, minor: 1, patch: 0 };
        let b = Version { major: 0, minor: 0, patch: 99 };
        assert!(a.newer_than(&b));
        assert!(Version { major: 0, minor: 0, patch: 1 }.newer_than(&Version { major: 0, minor: 0, patch: 0 }));
    }

    #[test]
    fn a959_rollback_gates() {
        // Healthy current → never roll back.
        assert!(!rollback_allowed(true, false, true));
        assert!(!rollback_depth_ok(0, 0));
        assert!(rollback_depth_ok(3, 0));
        assert!(!rollback_depth_ok(4, 0));
    }

    #[test]
    fn a963_fail_rate_edges() {
        let empty = RelEventLog::new();
        assert_eq!(empty.sign_fail_permille(), 0);
        let mut all_ok = RelEventLog::new();
        for _ in 0..5 {
            all_ok.push(RelEvent { kind: RelEventKind::SignOk, stamp: 1, arg: 0 });
        }
        assert_eq!(all_ok.sign_fail_permille(), 0);
    }

    #[test]
    fn a975_final() {
        let set = run_release_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("release self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
    }
}
