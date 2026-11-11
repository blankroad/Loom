import concurrent.futures,datetime,json,pathlib,urllib.request
ROOT=pathlib.Path(__file__).parent; OUT=ROOT/'docs-raw';OUT.mkdir(exist_ok=True)
SOURCES={
'visa-communication':'https://pyvisa.readthedocs.io/en/latest/introduction/communication.html',
'visa-binary':'https://pyvisa.readthedocs.io/en/latest/introduction/rvalues.html',
'visa-py-install':'https://pyvisa-py.readthedocs.io/en/latest/installation.html',
'visa-sim':'https://pyvisa-sim.readthedocs.io/en/latest/definitions.html',
'rtt':'https://kb.segger.com/RTT',
'jlink-os':'https://www.segger.com/downloads/jlink/',
'openocd-server':'https://openocd.org/doc/html/Server-Configuration.html',
'openocd-rtt':'https://openocd.org/doc/html/General-Commands.html',
'pyocd-docs':'https://pyocd.io/docs/',
'pyftdi-os':'https://eblot.github.io/pyftdi/installation.html',
'plin':'https://www.peak-system.com/products/software/development-packages/plin-api/?L=1',
'vector-xl':'https://www.vector.com/en/business-unit/testing-validation-diagnostics/products/network-interfaces/xl-driver-library/',
'saleae-api':'https://docs.saleae.com/automation/guides/getting-started',
'saleae-os':'https://www.saleae.com/support/logic-software/download-and-installation/supported-operating-systems',
'sigrok-bindings':'https://sigrok.org/wiki/Libsigrok',
'sigrok-windows':'https://sigrok.org/wiki/Windows',
'numato-prices':'https://numato.com/product-category/usb-relay-modules',
'ft232h-price':'https://www.adafruit.com/product/2264',
'jlink-price':'https://www.segger.com/products/debug-probes/j-link/models/j-link-base/',
'buspirate':'https://docs.buspirate.com/docs/binmode-reference/',
'xcp-quickstart':'https://pyxcp.readthedocs.io/en/latest/quickstart.html',
'xcp-simulator':'https://github.com/vectorgrp/XCPlite',
'qemu-gdb':'https://www.qemu.org/docs/master/system/gdb.html',
'renode-gdb':'https://renode.readthedocs.io/en/latest/debugging/gdb.html',
'someipy-docs':'https://someipy.readthedocs.io/en/latest/',
'keysight-scpi':'https://helpfiles.keysight.com/csg/e36300/Content/3_SCPI%20Programming/Introduction%20to%20SCPI.htm',
}
def one(kv):
 k,u=kv
 try:
  with urllib.request.urlopen(urllib.request.Request(u,headers={'User-Agent':'Mozilla/5.0'}),timeout=25) as r:t=r.read().decode(errors='replace');status=r.status
  (OUT/(k+'.txt')).write_text(t)
  return {'id':k,'url':u,'status':status,'file':str(OUT/(k+'.txt'))}
 except Exception as e:return {'id':k,'url':u,'error':str(e)}
with concurrent.futures.ThreadPoolExecutor(max_workers=6) as p:rows=list(p.map(one,SOURCES.items()))
(ROOT/'source-manifest.json').write_text(json.dumps({'retrieved_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sources':rows},ensure_ascii=False,indent=2))
for r in rows:print(r['id'],r.get('status',r.get('error')))
