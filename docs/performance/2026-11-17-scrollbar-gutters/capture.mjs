import {writeFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
const {chromium}=await import(pathToFileURL(process.argv[2]).href);
// Playwright normally injects --hide-scrollbars, which silently turns this
// allocation reference into an overlay-only experiment.
const browser=await chromium.launch({headless:true,ignoreDefaultArgs:['--hide-scrollbars']});
try {
  const page=await browser.newPage();
  await page.goto(new URL('browser-reference.html',import.meta.url).href);
  const result=await page.evaluate(()=>window.referenceReady);
  const probe=result.cases.find(c=>c.direction==='row'&&c.wrap==='nowrap'&&!c.rtl&&c.gutter===6&&c.overflow==='both'&&c.model==='border-box');
  if(!probe || probe.actual[2]!==64 || probe.actual[3]!==44) throw new Error('Browser did not reserve the requested classic scrollbar gutters');
  const fields=['direction','wrap','rtl','gutter','overflow','model'];
  await writeFile(new URL('browser-reference.json',import.meta.url),JSON.stringify({userAgent:result.userAgent,cases:result.cases.length,fields,tolerance:0.02},null,2)+'\n');
  await writeFile(new URL('browser-reference.tsv',import.meta.url),result.cases.map(c=>[...fields.map(k=>c[k]),...c.actual].join('\t')).join('\n')+'\n');
  console.log(JSON.stringify({userAgent:result.userAgent,cases:result.cases.length}));
} finally {await browser.close();}
