#!/bin/zsh
# bounded read-only poll of latest video frame metadata (id + sha256), no media bytes
L=$1; OUT=$2; END=$(( $(date +%s) + $3 ))
export LOOM_NO_AUTOSTART=1 LOOM_CLIENT=qa-native-video-poller
while [ $(date +%s) -lt $END ]; do
  $L video frame --capture v4e0216eadb82 --json 2>/dev/null | python3 -c 'import json,sys;d=json.load(sys.stdin);x=d.get("data") or {};f=x.get("frame",x);print(json.dumps({"id":f.get("frame_id"),"sha256":f.get("sha256"),"recv_utc_ns":f.get("recv_utc_ns"),"size":f.get("size")}))' >> $OUT
done
