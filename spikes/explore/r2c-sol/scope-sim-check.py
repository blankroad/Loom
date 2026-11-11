import pathlib,json,pyvisa,datetime
root=pathlib.Path(__file__).parent.resolve();rm=pyvisa.ResourceManager(str(root/'scope-sim.yaml')+'@sim');i=rm.open_resource('TCPIP::localhost::INSTR');i.timeout=500;i.read_termination='\n';i.write_termination='\n'
try:
 idn=i.query('*IDN?');i.write(':WAV:SOUR CHAN2');source=i.query(':WAV:SOUR?');values=i.query_binary_values(':WAV:DATA?',datatype='B');assert values==[0,1,2,3] and source=='CHAN2'
 result={'time':datetime.datetime.now(datetime.timezone.utc).isoformat(),'status':'pass','idn':idn,'source_after_write':source,'binary_block_values':values,'scope':'YAML static IEEE-488.2 block + mutable source property only; no trigger/timing/TCP/physical instrument'}
finally:i.close();rm.close()
(root/'scope-sim-result.json').write_text(json.dumps(result,indent=2));print(json.dumps(result,indent=2))
