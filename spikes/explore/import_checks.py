import datetime,importlib,json,pathlib,traceback
rows=[]
for name in ['pyvisa','pyvisa_py','pyvisa_sim','pymodbus','can','pydlt','someipy','asyncssh','pyocd','pylink','pyftdi','pygdbmi','saleae.automation','scapy.contrib.automotive.someip']:
 try:m=importlib.import_module(name);rows.append({'module':name,'status':'pass'})
 except Exception:rows.append({'module':name,'status':'fail','error':traceback.format_exc()})
pathlib.Path('spikes/explore/import-results.json').write_text(json.dumps({'retrieved_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'checks':rows},indent=2))
print(json.dumps(rows,indent=2))
