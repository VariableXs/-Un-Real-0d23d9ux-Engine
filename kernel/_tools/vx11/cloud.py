#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""VX11 云端通道：run/put/get/bg/log。凭据只从环境变量读取。"""
import os
import sys
import time

import paramiko


def client():
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(os.environ["CLOUD_SSH_HOST"], port=int(os.environ["CLOUD_SSH_PORT"]),
              username=os.environ["CLOUD_SSH_USER"], password=os.environ["CLOUD_SSH_PASSWORD"],
              timeout=30, banner_timeout=30, auth_timeout=30)
    return c


def run(cmd, timeout=3600):
    c = client()
    try:
        chan = c.get_transport().open_session(timeout=30)
        chan.settimeout(30)
        chan.exec_command("source ~/.cargo/env; " + cmd)
        out = b""
        err = b""
        t0 = time.time()
        while True:
            if chan.recv_ready():
                out += chan.recv(65536)
            if chan.recv_stderr_ready():
                err += chan.recv_stderr(65536)
            if chan.exit_status_ready() and not chan.recv_ready() and not chan.recv_stderr_ready():
                break
            if time.time() - t0 > timeout:
                out += b"[local-timeout]"
                break
            time.sleep(0.05)
        rc = chan.recv_exit_status() if chan.exit_status_ready() else -1
        sys.stdout.write(out.decode("utf-8", "replace"))
        if err:
            sys.stderr.write(err.decode("utf-8", "replace"))
        print("[rc=%d]" % rc)
        return rc
    finally:
        c.close()


def put(local, remote):
    c = client()
    try:
        sftp = c.open_sftp()
        c.exec_command("mkdir -p %s" % remote.rsplit("/", 1)[0])[1].read()
        sftp.put(local, remote)
        sftp.close()
        print("PUT OK", remote)
    finally:
        c.close()


def get(remote, local):
    c = client()
    try:
        sftp = c.open_sftp()
        sftp.get(remote, local)
        sftp.close()
        print("GET OK", local)
    finally:
        c.close()


def bg(cmd, logfile):
    c = client()
    try:
        chan = c.get_transport().open_session(timeout=30)
        chan.exec_command(
            "mkdir -p $(dirname %s); cd /root/varix-work/kernel && "
            "nohup setsid bash -c 'source ~/.cargo/env; %s' > %s 2>&1 < /dev/null & echo BG"
            % (logfile, cmd.replace("'", "'\\''"), logfile))
        out = b""
        t0 = time.time()
        while time.time() - t0 < 15:
            if chan.recv_ready():
                out += chan.recv(4096)
            if chan.exit_status_ready():
                break
            time.sleep(0.05)
        print(out.decode("utf-8", "replace").strip())
    finally:
        c.close()


if __name__ == "__main__":
    mode = sys.argv[1]
    if mode == "run":
        run(sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 3600)
    elif mode == "put":
        put(sys.argv[2], sys.argv[3])
    elif mode == "get":
        get(sys.argv[2], sys.argv[3])
    elif mode == "bg":
        bg(sys.argv[2], sys.argv[3])
