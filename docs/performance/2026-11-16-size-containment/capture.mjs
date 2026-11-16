// Usage: node capture.mjs <path-or-file-URL-to-playwright-core-index.mjs>
import {writeFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
const modulePath = process.argv[2];
if (!modulePath) throw new Error('Pass the installed playwright-core entry point');
const {chromium} = await import(modulePath.startsWith('file:') ? modulePath : pathToFileURL(modulePath).href);
const browser = await chromium.launch({headless:true});
try {
    const page = await browser.newPage();
    await page.goto(new URL('browser-reference.html', import.meta.url).href);
    const result = await page.evaluate(() => window.referenceReady);
    await writeFile(new URL('browser-reference.json', import.meta.url), JSON.stringify(result,null,2) + '\n');
    console.log(JSON.stringify({passed:result.passed,failed:result.failed,userAgent:result.userAgent}));
    if (result.failed) process.exitCode = 1;
} finally {
    await browser.close();
}
