import {writeFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
const {chromium}=await import(pathToFileURL(process.argv[2]).href);
const browser=await chromium.launch({headless:true});
try {
  const page=await browser.newPage();
  await page.goto(new URL('browser-reference.html',import.meta.url).href);
  const result=await page.evaluate(()=>window.referenceReady);
  const fields=['replaced','direction','cross','limit','model','constraint'];
  const deviations=result.cases.filter(c=>!c.replaced && c.direction.startsWith('row') && c.cross!==null && c.limit==='cross-max')
    .map(c=>({...c,expectedWidth:c.model==='content'?74:60}));
  await writeFile(new URL('browser-reference.json',import.meta.url),JSON.stringify({userAgent:result.userAgent,cases:result.cases.length,fields,tolerance:0.02,
    expectedMatches:result.cases.length-deviations.length,deviations,
    reason:'CSS Flexbox4.5 clamps the content size suggestion by definite cross limits transferred through the ratio; Chromium145 retains the natural width in these cases.'},null,2)+'\n');
  await writeFile(new URL('browser-reference.tsv',import.meta.url),result.cases.map(c=>[...fields.map(k=>c[k]??'auto'),...c.actual].join('\t')).join('\n')+'\n');
  console.log(JSON.stringify({userAgent:result.userAgent,cases:result.cases.length}));
} finally {await browser.close();}
