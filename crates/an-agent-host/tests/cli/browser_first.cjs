// Run with playwright-core on NODE_PATH and an operator-owned local product page.
const { chromium } = require('playwright-core');
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');

async function main() {
  const [chrome, endpoint, bundle, pdf, evidence] = process.argv.slice(2);
  fs.mkdirSync(evidence, { mode: 0o700 });
  const browser = await chromium.launch({ executablePath: chrome });
  try {
    const page = await browser.newPage({ viewport: { width: 1150, height: 850 } });
    page.setDefaultTimeout(300000);
    const control = id => page.locator('#' + id);
    const waitState = text => page.waitForFunction(expected =>
      document.getElementById('delivery-state').textContent.startsWith(expected), text);
    await page.goto(JSON.parse(fs.readFileSync(endpoint)).url);
    await control('bundle').setInputFiles(bundle);
    await control('inspect').click();
    await control('approve').click();
    await page.getByRole('button', { name: '确认准备环境', exact: true }).click();
    await page.reload();
    await page.waitForFunction(() => document.getElementById('setup-state').textContent === '可以运行');
    await page.screenshot({ path: path.join(evidence,'desktop.png') });
    if (await control('capacity').isVisible()) {
      await control('capacity').click();
      await control('capacity-confirm').click();
    }

    async function submit() {
      await control('delivery-file').setInputFiles(pdf);
      await control('delivery-preview').click();
      await control('delivery-confirm').click();
      await control('delivery-answer').waitFor({state:'visible'});
    }
    await submit();
    const task = await page.evaluate(() => sessionStorage.getItem('delivery:'+location.origin));
    await page.reload();
    await control('delivery-answer').waitFor({state:'visible'});
    assert.equal(await page.evaluate(() => sessionStorage.getItem('delivery:'+location.origin)),task);
    await page.setViewportSize({width:390,height:844});
    await control('language').selectOption('en');
    await page.waitForFunction(() => document.getElementById('clarification-form').dataset.language==='en');
    await control('clarification-form').scrollIntoViewIfNeeded();
    await page.screenshot({path:path.join(evidence,'mobile.png')});
    for (const input of await control('clarification-form').locator('input').all()) await input.fill('12');
    await control('delivery-answer').click();
    const products={};
    for(const [label,name] of [['Markdown','accepted.md'],['Excel','result.xlsx']]){
      const download=page.waitForEvent('download');
      const response=page.waitForResponse(r => r.request().method()==='POST' && r.url().endsWith('/action') && r.request().postDataJSON()?.op==='delivery-artifact');
      await control('delivery-products').getByRole('button',{name:new RegExp(label)}).click();
      const output=path.join(evidence,name);
      await (await download).saveAs(output);
      const bytes=fs.readFileSync(output);
      assert.deepEqual(bytes,Buffer.from((await (await response).json()).bytes));
      products[name]=crypto.createHash('sha256').update(bytes).digest('hex');
    }
    require('node:child_process').execFileSync('python3',['-c',`
import sys, zipfile, xml.etree.ElementTree as ET
from decimal import Decimal
with zipfile.ZipFile(sys.argv[1]) as workbook:
    sheet = ET.fromstring(workbook.read('xl/worksheets/sheet1.xml'))
cells = {cell.attrib['r']: cell for cell in sheet.findall('.//{*}c')}
assert Decimal(cells['D5'].findtext('{*}v')) == Decimal('38.50')
assert cells['D5'].findtext('{*}f') == 'SUM(D2:D4)'
for row in range(2,5):
    assert cells[f'D{row}'].findtext('{*}f') == f'B{row}*C{row}'
`,path.join(evidence,'result.xlsx')]);
    await submit();
    await control('delivery-cancel').click();
    const translations=await (await page.request.get(new URL('/strings.json',page.url()).href)).json();
    await waitState(translations.en.state_cancelled);
    assert.equal(await control('delivery-products').locator('button').count(),0);
    fs.writeFileSync(path.join(evidence,'report.json'),JSON.stringify({task,products,cancelled:true},null,2));
  } finally { await browser.close(); }
}
main().catch(error=>{console.error(error);process.exitCode=1;});
