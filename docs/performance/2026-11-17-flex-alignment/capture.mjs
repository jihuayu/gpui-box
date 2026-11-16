// Usage: node capture.mjs <installed-playwright-core-entry>
import {writeFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
const entry = process.argv[2];
if (!entry) throw new Error('Pass the installed playwright-core entry point');
const {chromium} = await import(entry.startsWith('file:') ? entry : pathToFileURL(entry).href);
const browser = await chromium.launch({headless:true});
try {
  const page = await browser.newPage();
  await page.goto(new URL('browser-reference.html', import.meta.url).href);
  const result = await page.evaluate(() => window.referenceReady);
  const fields = ['kind','dir','rtl','wrap','itemRtl','position','overflow','childW','childH','count'];
  const deviations = result.cases.filter(c => c.position==='space-between' && c.overflow==='default'
    && ((c.kind==='main' && c.dir.endsWith('reverse') && (c.dir.startsWith('row')?c.childW* c.count>100:c.childH*c.count>60))
      || (c.kind==='content' && c.wrap==='wrap-reverse' && (c.dir.startsWith('row')?c.childH*c.count>60:c.childW*c.count>100))));
  await writeFile(new URL('browser-reference.json',import.meta.url),JSON.stringify({userAgent:result.userAgent,cases:result.cases.length,fields,
    geometryFile:'browser-reference.tsv',tolerance:0.02,expectedMatches:result.cases.length-deviations.length,
    deviations,reason:'Chromium 145 uses flex-start for overflowing reverse space-between; current CSS requires safe flex-start -> logical start.'},null,2)+'\n');
  const lines = result.cases.map(c => [...fields.map(k=>c[k]),...c.actual.flat()].join('\t'));
  await writeFile(new URL('browser-reference.tsv',import.meta.url),lines.join('\n')+'\n');
  console.log(JSON.stringify({cases:result.cases.length,userAgent:result.userAgent}));
} finally { await browser.close(); }
