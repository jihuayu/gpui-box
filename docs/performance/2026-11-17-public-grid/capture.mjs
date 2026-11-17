import {writeFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
const {chromium}=await import(pathToFileURL(process.argv[2]).href);
const browser=await chromium.launch({headless:true});
try {
  const page=await browser.newPage();
  await page.goto(new URL('browser-reference.html',import.meta.url).href);
  const result=await page.evaluate(()=>window.referenceReady);
  const fields=['template','flow','rtl','width','placed','align','update'];
  await writeFile(new URL('browser-reference.json',import.meta.url),JSON.stringify({userAgent:result.userAgent,cases:result.cases.length,fields,tolerance:0.06},null,2)+'\n');
  await writeFile(new URL('browser-reference.tsv',import.meta.url),result.cases.map(c=>[...fields.map(k=>c[k]),...c.actual].join('\t')).join('\n')+'\n');
  console.log(JSON.stringify({userAgent:result.userAgent,cases:result.cases.length}));
} finally {await browser.close();}
