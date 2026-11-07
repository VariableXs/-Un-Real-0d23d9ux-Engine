import paramiko, sys, base64
HOST="connect.cqa1.seetacloud.com"; PORT=27347; USER="root"; PWD="cfOUZGl73ryw"

def cli():
    c=paramiko.SSHClient(); c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(HOST,port=PORT,username=USER,password=PWD,timeout=30,banner_timeout=30,auth_timeout=30)
    return c

def sh(c,cmd,timeout=1200,quiet=False):
    i,o,e=c.exec_command(cmd,timeout=timeout)
    out=o.read().decode(errors="replace"); err=e.read().decode(errors="replace")
    rc=o.channel.recv_exit_status()
    if not quiet:
        sys.stdout.write(out)
        if err: sys.stderr.write(err[-3000:])
        print("\nRC",rc)
    return rc,out,err

def push(c,local,remote):
    data=base64.b64encode(open(local,"rb").read())
    cmd="base64 -d > %s" % remote.replace("'","'\\''")
    cmd="base64 -d > '%s'" % remote.replace("'","'\\''")
    i,o,e=c.exec_command(cmd)
    i.write(data); i.channel.shutdown_write()
    rc=o.channel.recv_exit_status()
    err=e.read().decode(errors="replace")
    if rc!=0: raise RuntimeError("push failed rc=%d %s" % (rc,err[-500:]))
    print("PUT",local,"->",remote,len(data))

if __name__=="__main__":
    mode=sys.argv[1]
    c=cli()
    if mode=="put":
        a=sys.argv[2:]
        for k in range(0,len(a),2):
            push(c,a[k],a[k+1])
    elif mode=="run":
        sh(c,sys.argv[2],int(sys.argv[3]) if len(sys.argv)>3 else 1200)
    c.close()
