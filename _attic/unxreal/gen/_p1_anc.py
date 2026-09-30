import json,subprocess,urllib.request,base64,os,sys
sys.path.insert(0,"_attic/tools")
import gh_api_push as g
c=g.api("GET","git/commits/d793bd4") if False else None
