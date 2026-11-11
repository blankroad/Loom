import { build } from 'esbuild';
import fs from 'node:fs/promises';
await build({entryPoints:['client.js'],bundle:true,minify:true,format:'esm',outfile:'dist/client.js'});
await fs.copyFile('node_modules/@xterm/xterm/css/xterm.css','dist/xterm.css');
