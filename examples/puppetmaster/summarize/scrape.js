const puppeteer = require('puppeteer');
const gpt3 = require('gpt-3-encoder');

async function scrapeWebsite(url) {
    const browser = await puppeteer.launch({ headless: false });
    const page = await browser.newPage();
    await page.goto(url);
    await page.waitForSelector('body');
    await page.evaluate(async () => {
        await new Promise(resolve => {
            let totalHeight = 0;
            const distance = 100;
            const timer = setInterval(() => {
                const scrollHeight = document.body.scrollHeight;
                window.scrollBy(0, distance);
                totalHeight += distance;

                if (totalHeight >= scrollHeight) {
                    clearInterval(timer);
                    resolve();
                }
            }, 100);
        });
    });
    const paragraphs = (await page
        .$$eval('body p', paragraphs => paragraphs.map(p => p.textContent)))
        .join('\n\n');
    const encoded = gpt3.encode(paragraphs);

    console.log(gpt3.decode(encoded.slice(0, 2000)));
        
    await browser.close();
}

scrapeWebsite(process.argv[2]);
