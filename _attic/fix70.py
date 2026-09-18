import io
p = 'kernel/varix/tests/fuzz_parsers.rs'
s = io.open(p, encoding='utf-8').read()

# 1. ExFat → ExfatVolume；run_guarded 签名简化为 fn 指针（去 transmute）
s = s.replace('exfat_ro::ExFat::mount(disk)', 'exfat_ro::ExfatVolume::mount(disk)')

# 2. 非法十六进制笔误
s = s.replace('let mut rng = Lcg(0xVAC0DE as u64 | 0xF00D);\n    let _ = &mut rng;\n', 'let mut rng = Lcg(0xBEEF_C0DE_F00D_1234);\n')

# 3. run_guarded：fn 指针天然 'static，去 transmute
old = """/// 单次执行封装：独立线程 + 挂死超时 + panic 归档。返回 (ok, executed)。
fn run_guarded(name: &'static str, n: usize, input: &[u8], f: &dyn Fn(&[u8])) -> (bool, bool) {
    let (tx, rx) = mpsc::channel();
    let inp = input.to_vec();
    let fptr: &'static dyn Fn(&[u8]) = unsafe { std::mem::transmute(f) };
    let h = std::thread::Builder::new()
        .stack_size(4 << 20) // 4MiB：parse_imports 返回值物化 ~41KB×2 富余。
        .spawn(move || {
            let ok = catch_unwind(AssertUnwindSafe(|| fptr(&inp))).is_ok();
            let _ = tx.send(ok);
        })
        .expect("spawn");
    match rx.recv_timeout(Duration::from_millis(HANG_TIMEOUT_MS)) {
        Ok(ok) => (ok, true),
        Err(_) => {
            // 挂死：归档样本（线程泄漏给进程退出回收）。
            archive(name, n, input, "hang");
            (false, false)
        }
    }
    #[allow(unused)]
    drop(h);
}"""
new = """/// 单次执行封装：独立线程 + 挂死超时 + panic 归档。返回 (ok, ran)。
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
}"""
assert old in s, 'run_guarded block'
s = s.replace(old, new, 1)

# 4. replay_corpus 签名同步 fn 指针
s = s.replace('fn replay_corpus(name: &\'static str, f: &dyn Fn(&[u8])) -> usize {',
              'fn replay_corpus(name: &\'static str, f: fn(&[u8])) -> usize {')

io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('OK')
