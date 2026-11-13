const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {JSDOM}=require('../../../../ui/node_modules/jsdom');
const css=fs.readFileSync(path.join(__dirname,'styles.css'),'utf8'),html=fs.readFileSync(path.join(__dirname,'index.html'),'utf8');
const dom=new JSDOM(html.replace('</head>','<style>'+css+'</style></head>'));
const w=dom.window,shell=w.document.querySelector('.shell');
assert.equal(w.getComputedStyle(shell).gridTemplateColumns.replace(/\s/g,'').replaceAll('0px','0'),'minmax(0,1fr)');
for(const child of shell.children){assert.equal(w.getComputedStyle(child).minWidth.replace('px',''),'0');assert.equal(w.getComputedStyle(child).maxWidth,'100%');}
assert.equal(w.document.querySelector('#socket-work').hidden,true);
assert(css.includes('[hidden]{display:none!important}'));
assert(css.includes('#channel-list{display:flex;overflow:auto;'));
assert(!/\b(?:html|body)\s*\{[^}]*overflow(?:-x)?\s*:\s*hidden/.test(css));
console.log('PASS F2 shell: explicit minmax(0,1fr) outer track and shrinkable bounded direct children.');
console.log('PASS F2 shell: inactive socket pane hidden; channel rail retains internal overflow; no document overflow mask.');
console.log('CSS/DOM source assertions only. ROOT native 390px rendered scrollWidth remains the acceptance test.');w.close();
