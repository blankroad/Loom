const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {JSDOM}=require('../../../../ui/node_modules/jsdom');
function setup(width){const dom=new JSDOM(fs.readFileSync(path.join(__dirname,'index.html'),'utf8'),{url:'http://localhost:4180/ethernet-workbench/sol/index.html',runScripts:'outside-only',pretendToBeVisual:true});const w=dom.window;w.TextEncoder=TextEncoder;w.setInterval=()=>1;Object.defineProperty(w,'innerWidth',{value:width,writable:true});for(const f of ['engine.js','app.js'])w.eval(fs.readFileSync(path.join(__dirname,f),'utf8'));return dom;}
const dom=setup(1440),w=dom.window,$=s=>w.document.querySelector(s),inspection=$('#inspection'),divider=$('#vertical-divider');
inspection.getBoundingClientRect=()=>({width:w.innerWidth>600?w.innerWidth-(w.innerWidth>1000?210:176):w.innerWidth,left:210});divider.setPointerCapture=()=>{};divider.releasePointerCapture=()=>{};
divider.onpointerdown({clientX:766,pointerId:1,preventDefault(){}});divider.onpointermove({clientX:859});divider.onpointerup({pointerId:1});assert.equal(parseFloat(inspection.style.gridTemplateColumns),649);
w.innerWidth=768;w.dispatchEvent(new w.Event('resize'));assert.equal(parseFloat(inspection.style.gridTemplateColumns),336);assert(parseFloat(inspection.style.gridTemplateColumns)+6+250<=592);
w.innerWidth=390;w.dispatchEvent(new w.Event('resize'));assert.equal(inspection.style.gridTemplateColumns,'');assert.equal($('#packet-workspace').style.gridTemplateRows,'');
console.log('PASS F2: desktop pointer split at 649px clamps to 336 + 6 + 250 inside 592px at viewport768, then removes pixel columns at390.');
const fresh=setup(390);fresh.window.dispatchEvent(new fresh.window.Event('resize'));assert.equal(fresh.window.document.querySelector('#inspection').style.gridTemplateColumns,'');
const css=fs.readFileSync(path.join(__dirname,'styles.css'),'utf8');assert(css.includes('#capture-work,#packet-workspace,.packet-pane,.packet-scroll,#socket-work,#channel-list{min-width:0;max-width:100%}'));assert(/\.packet-scroll\{overflow:auto/.test(css));assert(/#packet-bytes\{overflow:auto/.test(css));assert(/#packet-detail\{overflow:auto/.test(css));assert(css.includes('#inspection{display:flex;flex-direction:column}'));
console.log('PASS F2: fresh390 has no pixel split; table/grid/flex children shrink with pane-local scrolling, mobile inspection stacks.');
console.log('Source-local resize math and CSS containment checks only. Native scrollWidth/layout retest belongs to ROOT.');w.close();fresh.window.close();
