import socket, sys, time
port = int(sys.argv[1])
cmd = sys.argv[2].encode() + b"\n"
s = socket.create_connection(("127.0.0.1", port), timeout=5)
time.sleep(0.5)
s.recv(4096)
s.sendall(cmd)
time.sleep(1.0)
try:
    data = s.recv(65536)
    print(data.decode(errors="replace"))
except Exception as e:
    print("err", e)
s.close()
