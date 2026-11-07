"""Upload a set of unxstar modules to an isolated cloud probe crate and run checks.

Usage:
    python _vx11_check_remote.py <tag> <num> [<num> ...]

The helper keeps one probe crate per tag (/root/vx11probe_<tag>) so parallel
agents never share build state. The first num is also always compiled (its
module may be a shared dependency for the rest of the group).
"""
import glob
import io
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, r"C:\Users\varia\AppData\Local\Temp")
from cloud_sync import cli, push, sh  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
MODDIR = os.path.join(HERE, "..", "varix", "src", "unxstar")


def local_file(num):
    hits = glob.glob(os.path.join(MODDIR, "unx%s_*.rs" % num))
    if not hits:
        raise SystemExit("missing module for %s" % num)
    return hits[0]


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    tag = sys.argv[1]
    nums = sys.argv[2:]
    remote = "/root/vx11probe_%s" % tag
    c = cli()
    sh(c, "mkdir -p %s/src/mods" % remote, quiet=True)
    sh(c, "cp -f /root/probe/Cargo.toml %s/Cargo.toml && cp -f /root/probe/src/lib.rs %s/src/lib.rs && cp -f /root/probe/src/checks.rs %s/src/checks.rs" % (remote, remote, remote), quiet=True)
    stems = []
    for n in nums:
        p = local_file(n)
        stem = os.path.basename(p)[:-3]
        stems.append(stem)
        push(c, p, "%s/src/mods/%s.rs" % (remote, stem))
    mods = "".join("pub mod %s;\n" % s for s in stems)
    body = ["fn main() {"]
    body.append("    let mut total = 0usize; let mut red = 0usize;")
    for n, s in zip(nums, stems):
        body.append('    { let s = probe::mods::%s::run_unx%s_checks();' % (s, n))
        body.append('      if !s.all_passed() { red += 1; println!("RED %s :: {}", s.red_items_text()); }' % n)
        body.append("      total += 1; }")
    body.append('    println!("TOTAL={} RED={}", total, red);')
    body.append("}")
    push_text(c, mods, "%s/src/mods.rs" % remote)
    push_text(c, "\n".join(body), "%s/src/main.rs" % remote)
    rc, out, err = sh(c, "source $HOME/.cargo/env; cd %s && timeout 600 cargo run -q --bin probe 2>&1 | tail -60" % remote, timeout=700)
    if "TOTAL=" not in out:
        c.close()
        sys.stderr.write("BUILD/RUN FAILED\n")
        sys.exit(2)
    _, tout, _ = sh(c, "source $HOME/.cargo/env; cd %s && timeout 600 cargo test --lib -q 2>&1 | tail -6" % remote, timeout=700)
    c.close()
    if "test result: ok" not in tout:
        sys.stderr.write("TESTS FAILED\n")
        sys.exit(3)


def push_text(c, text, remote):
    data = text.encode("utf-8")
    import base64
    b64 = base64.b64encode(data)
    cmd = "base64 -d > '%s'" % remote
    i, o, e = c.exec_command(cmd)
    i.write(b64)
    i.channel.shutdown_write()
    rc = o.channel.recv_exit_status()
    if rc != 0:
        raise SystemExit("push_text failed: %s" % e.read().decode(errors="replace"))


if __name__ == "__main__":
    main()
