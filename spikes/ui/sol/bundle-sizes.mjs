import {build} from 'esbuild';
import {compile} from 'svelte/compiler';
import {gzipSync} from 'node:zlib';
import fs from 'node:fs/promises';
const cases={
  react:'import React,{useState} from "react";import{createRoot}from"react-dom/client";function App(){const[n,set]=useState(0);return React.createElement("button",{onClick:()=>set(n+1)},n)}createRoot(document.body).render(React.createElement(App));',
  preact:'import{h,render}from"preact";import{useState}from"preact/hooks";function App(){const[n,set]=useState(0);return h("button",{onClick:()=>set(n+1)},n)}render(h(App),document.body);',
  solid:'import{createSignal,createEffect}from"solid-js";import{render}from"solid-js/web";render(()=>{const[n,set]=createSignal(0);const b=document.createElement("button");b.onclick=()=>set(n()+1);createEffect(()=>b.textContent=n());return b},document.body);',
  svelte:compile('<script>let n=$state(0)</script><button onclick={()=>n++}>{n}</button>',{generate:'client',dev:false,filename:'Counter.svelte'}).js.code+'\nimport{mount}from"svelte";mount(Counter,{target:document.body});'
};
const rows=[];
for(const[name,contents]of Object.entries(cases)){
  const r=await build({stdin:{contents,resolveDir:process.cwd(),sourcefile:name+'.js'},bundle:true,minify:true,write:false,format:'esm',platform:'browser',conditions:['browser'],define:{'process.env.NODE_ENV':'"production"'}});
  const b=r.outputFiles[0].contents;rows.push({name,raw_bytes:b.length,gzip_bytes:gzipSync(b).length});
}
await fs.mkdir('results',{recursive:true});
await fs.writeFile('results/bundle-sizes.json',JSON.stringify({method:'One mounted counter, esbuild 0.28.2 production/minified ESM, gzip default. Not full Loom app; compiler toolchain excluded; JSX-free equivalent Solid DOM.',rows},null,2));
console.log(rows);
