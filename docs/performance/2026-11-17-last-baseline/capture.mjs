import {writeFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
const entry=process.argv[2]; if(!entry)throw new Error('Pass installed playwright-core entry');
const {chromium}=await import(entry.startsWith('file:')?entry:pathToFileURL(entry).href);
const browser=await chromium.launch({headless:true});
try {
  const page=await browser.newPage();await page.goto(new URL('browser-reference.html',import.meta.url).href);
  const r=await page.evaluate(()=>window.referenceReady);
  const rows=r.cases.map(c=>[c.dir,c.rtl,c.wrap,c.mode,c.extent,c.variant,c.width,c.height??'auto',...c.actual,
    ...c.items.flatMap(i=>[i.align,i.mt,i.mb,i.ml,i.mr,i.first,i.last,...i.actual])].join('\t'));
  await writeFile(new URL('browser-reference.tsv',import.meta.url),rows.join('\n')+'\n');
  await writeFile(new URL('container-baselines.tsv',import.meta.url),r.cases.map(c=>c.exported.join('\t')).join('\n')+'\n');
  await writeFile(new URL('browser-reference.json',import.meta.url),JSON.stringify({userAgent:r.userAgent,cases:r.cases.length,
    rootFields:['direction','rtl','wrap','mode','extent','variant','width','height','actualWidth','actualHeight'],
    itemFields:['alignment','marginTop','marginBottom','marginLeft','marginRight','firstBaseline','lastBaseline','x','y','width','height'],items:4},null,2)+'\n');
  console.log(JSON.stringify({userAgent:r.userAgent,cases:r.cases.length}));
}finally{await browser.close();}
