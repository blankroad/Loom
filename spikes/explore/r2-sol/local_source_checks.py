import hashlib,json,pathlib,re
ROOT=pathlib.Path(__file__).parent
base=pathlib.Path('/Users/ctmctm/Desktop/01_PROJECT/dlt-wrapper')
files={
'local-dlt-common':base/'third_party/dlt/orig/include/dlt/dlt_common.h',
'local-dlt-protocol':base/'third_party/dlt/orig/include/dlt/dlt_protocol.h',
'local-dlt-client':base/'build/dlt/orig/src/src/lib/dlt_client.c',
'local-dlt-daemon-client':base/'build/dlt/orig/src/src/daemon/dlt_daemon_client.c',
'local-dlt-common-c':base/'build/dlt/orig/src/src/shared/dlt_common.c',
'local-dlt-daemon-connection':base/'build/dlt/orig/src/src/daemon/dlt_daemon_connection.c',
}
rows=[]
for name,p in files.items():
 text=p.read_text();lines=text.splitlines();matched=set()
 for i,l in enumerate(lines):
  if re.search(r'dlt_client_send_marker|DltServiceMarker|dlt_daemon_control_message_marker|dlt_set_storageheader|DLT0x01|without storage header|dltSerialHeader\[|dlt_daemon_connection_create|DLT_SERVICE_ID_SET_ALL_LOG_LEVEL',l):
   matched.update(range(max(0,i-3),min(len(lines),i+23)))
 (ROOT/(name+'-excerpts.txt')).write_text('\n'.join(f'{i+1}: {lines[i]}' for i in sorted(matched)))
 rows.append({'name':name,'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'lines':len(lines),'excerpt_file':name+'-excerpts.txt'})
(ROOT/'local-source-manifest.json').write_text(json.dumps(rows,indent=2))
