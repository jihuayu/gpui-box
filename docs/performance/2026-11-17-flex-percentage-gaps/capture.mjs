import {writeFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
const {chromium}=await import(pathToFileURL(process.argv[2]).href);
const browser=await chromium.launch({headless:true});
try {
 const page=await browser.newPage();
 await page.goto(new URL('browser-reference.html',import.meta.url).href);
 const result=await page.evaluate(()=>window.referenceReady);
 const fields=['direction','wrap','main','cross','model','gap'];
 const deviations=result.cases.filter(c=>c.direction.startsWith('column')&&c.wrap!=='nowrap'&&c.main==='min-content').map(c=>({...c,reason:'CSS Flexbox9.9.1.3 specifies the largest item contribution for multi-line min-content main size; Chromium145 uses the unwrapped block extent. These72 cases are asserted separately against the normative geometry.'}));
 await writeFile(new URL('browser-reference.json',import.meta.url),JSON.stringify({userAgent:result.userAgent,cases:result.cases.length,fields,tolerance:0.02,expectedMatches:result.cases.length-deviations.length,deviations},null,2)+'\n');
 await writeFile(new URL('browser-reference.tsv',import.meta.url),result.cases.map(c=>[...fields.map(k=>c[k]),...c.actual].join('\t')).join('\n')+'\n');
 console.log(JSON.stringify({userAgent:result.userAgent,cases:result.cases.length}));
} finally {await browser.close();}
