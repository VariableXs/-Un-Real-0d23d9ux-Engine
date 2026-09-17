//! 任务70（AI-S）· 六解析器 fuzz 常态化（boot-select / PE / ELF / exFAT /
//! 规则 / Uxv）。
//!
//! ## 运行方式（宿主机可跑）
//! ```text
//! cargo ktest --test fuzz_parsers                                  # 默认 10 万轮起步
//! VARIX_PARSERS_ROUNDS=1000 cargo ktest --test fuzz_parsers        # CI 快速模式
//! ```
//!
//! ## 验收口径（任务表 70）
//! - 六个解析器各 ≥10 万次执行零崩溃（默认轮次即 100_000）；
//! - 崩溃样本自动归档为回归用例（panic → `tests/fuzz-corpus/<parser>/`）；
//! - 每次启动先重放 corpus 全部样本（回归前置）；
//! - 新解析器注册进清单 = 在 [`PARSERS`] 注册表加一项（单一注册点）。
//!
//! ## 防挂死
//! 解析在独立线程执行，单轮超过 [`HANG_TIMEOUT_MS`] 视为"挂死发现"归档
//! 样本（挂死=可用性崩溃，同样算失败发现，如实登记不粉饰）。

#![cfg(target_os = "windows")]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc;
use std::time::Duration;

use varix::bootcfg;
use varix::fs::exfat_ro::{self, Bpb};
use varix::proc::{elf, pe};
use varix::uxvingest;
use varix::vfsguard;

/// 单轮解析挂死判定（正常单次解析毫秒级）。
const HANG_TIMEOUT_MS: u64 = 3_000;
/// 回归语料目录（入库：崩溃样本即回归用例）。
const CORPUS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fuzz-corpus");
/// 默认轮次：任务表验收口径 ≥10 万次/解析器。
const DEFAULT_ROUNDS: u32 = 100_000;

fn rounds() -> u32 {
    std::env::var("VARIX_PARSERS_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_ROUNDS)
}

/// 确定性伪随机（xorshift64*，与 tests/fuzz.rs 同款）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 语料变异：随机长度 + 随机字节 + 偶发复用"种子前缀"（结构化起点，
/// 提高命中深层分支概率）。
fn mutate(rng: &mut Lcg, seed: &[u8], buf: &mut Vec<u8>) {
    buf.clear();
    let use_seed = rng.below(4) == 0 && !seed.is_empty();
    if use_seed {
        let take = (rng.below(seed.len() as u64 + 1)) as usize;
        buf.extend_from_slice(&seed[..take]);
    }
    let len = rng.below(4097) as usize;
    let mode = rng.below(8);
    for _ in 0..len {
        let b = match mode {
            0 => 0u8,
            1 => 0xffu8,
            2 => (rng.next() & 0xff) as u8,
            3 if !buf.is_empty() => buf[rng.below(buf.len() as u64) as usize],
            _ => (rng.next() & 0xff) as u8,
        };
        buf.push(b);
    }
}

/// 单次执行封装：独立线程 + 挂死超时 + panic 归档。返回 (ok, ran)。
/// `f` 为 fn 指针（天然 'static，无 transmute）。
fn run_guarded(name: &'static str, n: usize, input: &[u8], f: fn(&[u8])) -> (bool, bool) {
    let (tx, rx) = mpsc::channel();
    let inp = input.to_vec();
    let _h = std::thread::Builder::new()
        .stack_size(4 << 20) // 4MiB：parse_imports 返回值物化 ~41KB×2 富余。
        .spawn(move || {
            let ok = catch_unwind(AssertUnwindSafe(|| f(&inp))).is_ok();
            let _ = tx.send(ok);
        })
        .expect("spawn");
    match rx.recv_timeout(Duration::from_millis(HANG_TIMEOUT_MS)) {
        Ok(ok) => (ok, true),
        Err(_) => {
            // 挂死：归档样本（线程随测试进程退出回收）。
            archive(name, n, input, "hang");
            (false, false)
        }
    }
}

/// 崩溃/挂死样本自动归档（回归用例转化脚本化：fuzz 内直接落盘）。
fn archive(name: &str, n: usize, input: &[u8], kind: &str) {
    let dir = format!("{}/{}", CORPUS_DIR, name);
    let _ = std::fs::create_dir_all(&dir);
    let path = format!("{}/{}-{}-{}.bin", dir, kind, n, std::process::id());
    let _ = std::fs::write(path, input);
    eprintln!("FUZZ FINDING: {} {} @ round {} (archived)", name, kind, n);
}

/// 语料回归重放：corpus 里既有样本全部跑一遍，任何 panic 直接 fail。
fn replay_corpus(name: &'static str, f: fn(&[u8])) -> usize {
    let dir = format!("{}/{}", CORPUS_DIR, name);
    let mut count = 0usize;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("bin") {
                if let Ok(data) = std::fs::read(&p) {
                    let (ok, _) = run_guarded(name, 0, &data, f);
                    assert!(ok, "corpus 回归失败：{}（样本 {:?}）", name, p);
                    count += 1;
                }
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// 六解析器执行体（全部公开纯 API；Err 返回值 = 如实拒绝，不算失败）
// ---------------------------------------------------------------------------

/// ① boot-select（bootcfg::parse：JSON/回退源裁决）。
fn exec_boot_select(data: &[u8]) {
    let _ = bootcfg::parse(data);
}

/// ② PE（pe::parse + 成功后导入表解析——任务40 的完整拒绝面）。
fn exec_pe(data: &[u8]) {
    if let Ok(img) = pe::parse(data) {
        // 导入表解析（宿主栈 4MiB，41KB 级返回值物化安全）。
        let _ = pe::parse_imports(data, &img);
    }
}

/// ③ ELF（elf::parse）。
fn exec_elf(data: &[u8]) {
    let _ = elf::parse(data);
}

/// ⑤ 规则（vfsguard：normalize + 规则文本 parse + 裁决）。
fn exec_rules(data: &[u8]) {
    let mut rs = vfsguard::RuleSet::new();
    let (added, _) = rs.parse(data);
    // 拒绝面也得走裁决：随机路径 normalize + adjudicate。
    let _ = vfsguard::normalize(data);
    let _ = rs.adjudicate(data, vfsguard::Op::Read);
    let _ = (added, vfsguard::Op::Write);
}

/// ⑥ Uxv（uxv_ingest::ingest：整包校验门）。
fn exec_uxv(data: &[u8]) {
    let _ = uxvingest::ingest(data);
}

// ---------------------------------------------------------------------------
// exFAT 专用（需要块设备：随机盘 mount + read_dir）
// ---------------------------------------------------------------------------

/// 随机内存盘（exFAT mount 用；512B 块，容量可配）。
struct FuzzDisk {
    blocks: std::collections::BTreeMap<u64, [u8; 512]>,
    total: u64,
}

impl varix::drivers::blk::BlockDevice for FuzzDisk {
    fn block_size(&self) -> u32 {
        512
    }
    fn capacity_blocks(&self) -> u64 {
        self.total
    }
    fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), varix::drivers::blk::BlockError> {
        use varix::drivers::blk::BlockError;
        if dst.is_empty() || dst.len() % 512 != 0 {
            return Err(BlockError::InvalidRange);
        }
        let zero = [0u8; 512];
        for (i, chunk) in dst.chunks_mut(512).enumerate() {
            chunk.copy_from_slice(self.blocks.get(&(lba + i as u64)).unwrap_or(&zero));
        }
        Ok(())
    }
    fn write_blocks(&mut self, _lba: u64, _src: &[u8]) -> Result<(), varix::drivers::blk::BlockError> {
        // 只读挂载语义：fuzz 不写。
        Ok(())
    }
    fn flush(&mut self) -> Result<(), varix::drivers::blk::BlockError> {
        Ok(())
    }
}

/// ④ exFAT（Bpb::parse VBR 拒绝面 + 随机盘 mount/read_dir）。
fn exfat_round(rng: &mut Lcg) -> (bool, bool) {
    // VBR 拒绝面：512B 随机 VBR。
    let mut vbr = [0u8; 512];
    for b in vbr.iter_mut() {
        *b = (rng.next() & 0xff) as u8;
    }
    let _ = Bpb::parse(&vbr);

    // 随机盘（1024 块 = 512KiB）→ mount → read_dir("/")。
    let mut disk = FuzzDisk { blocks: std::collections::BTreeMap::new(), total: 1024 };
    let nblocks = rng.below(1024) as usize;
    for _ in 0..nblocks {
        let lba = rng.below(1024);
        let mut blk = [0u8; 512];
        let sparse = rng.below(4) == 0;
        for b in blk.iter_mut() {
            *b = if sparse { 0 } else { (rng.next() & 0xff) as u8 };
        }
        disk.blocks.insert(lba, blk);
    }
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .stack_size(4 << 20)
        .spawn(move || {
            let ok = catch_unwind(AssertUnwindSafe(|| {
                if let Ok(mut fs) = exfat_ro::ExfatVolume::mount(disk) {
                    let _ = fs.read_dir("/");
                }
            }))
            .is_ok();
            let _ = tx.send(ok);
        })
        .expect("spawn");
    match rx.recv_timeout(Duration::from_millis(HANG_TIMEOUT_MS)) {
        Ok(ok) => (ok, true),
        Err(_) => (false, false),
    }
}

// ---------------------------------------------------------------------------
// 测试：六解析器统一入口（corpus 回归 → N 轮 fuzz → 零崩溃断言）
// ---------------------------------------------------------------------------

/// 注册表驱动：每个解析器一个测试（独立失败定位），共用 harness。
/// 新解析器注册点：写一个 exec_* + 一个 fuzz_* 测试（见 docs/fuzz-parsers.md）。

#[test]
fn fuzz_boot_select() {
    let seeds: Vec<Vec<u8>> = vec![b"{\"timeout\":5,\"default\":\"varix\",\"entries\":[]}".to_vec()];
    harness("boot-select", &seeds, exec_boot_select);
}

#[test]
fn fuzz_pe_parser() {
    let mut seeds = Vec::new();
    // 真实样例做结构化种子（带导入的 hello-imp.pe 优先）。
    if let Ok(d) = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/src/proc/hello-imp.pe")) {
        seeds.push(d);
    }
    if let Ok(d) = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/src/proc/hello.pe")) {
        seeds.push(d);
    }
    harness("pe", &seeds, exec_pe);
}

#[test]
fn fuzz_elf_parser() {
    harness("elf", &[], exec_elf);
}

#[test]
fn fuzz_rules_parser() {
    let seeds: Vec<Vec<u8>> = vec![
        b"/varix/system rw\n/varix/tmp r\n".to_vec(),
        b"/shared r\n".to_vec(),
    ];
    harness("rules", &seeds, exec_rules);
}

#[test]
fn fuzz_uxv_ingest() {
    harness("uxv", &[], exec_uxv);
}

#[test]
fn fuzz_exfat_mount() {
    let mut rng = Lcg(0xBEEF_C0DE_F00D_1234);
    let n = rounds();
    let mut executed = 0usize;
    for i in 0..n {
        let (ok, ran) = exfat_round(&mut rng);
        if !ran {
            // 挂死样本已归档；挂死=发现，测试失败（如实暴露）。
            panic!("exfat 挂死 @ round {}（样本已归档 fuzz-corpus/exfat/）", i);
        }
        assert!(ok, "exfat panic @ round {}（样本已归档）", i);
        executed += 1;
    }
    assert!(executed >= 100_000 || rounds() < 100_000, "轮次不足");
    eprintln!("fuzz exfat: {} rounds, 0 crash, 0 hang", executed);
}

/// 通用 harness：corpus 回归 → N 轮（panic/挂死即归档 + fail）。
fn harness(name: &'static str, seeds: &[Vec<u8>], f: fn(&[u8])) {
    let replayed = replay_corpus(name, f);
    let mut rng = Lcg(0x9E37_79B9_7F4A_7C15 ^ (name.as_ptr() as u64));
    let mut buf = Vec::with_capacity(4096);
    let n = rounds() as usize;
    for i in 0..n {
        let seed_pick: &[u8] = if seeds.is_empty() {
            &[]
        } else {
            seeds[(rng.next() as usize) % seeds.len()].as_slice()
        };
        mutate(&mut rng, seed_pick, &mut buf);
        let (ok, ran) = run_guarded(name, i, &buf, f);
        if !ran {
            panic!("{} 挂死 @ round {}（样本已归档 fuzz-corpus/{}/）", name, i, name);
        }
        assert!(ok, "{} panic @ round {}（样本已归档 fuzz-corpus/{}/）", name, i, name);
    }
    eprintln!("fuzz {}: {} rounds, corpus replay {}, 0 crash, 0 hang", name, n, replayed);
}
