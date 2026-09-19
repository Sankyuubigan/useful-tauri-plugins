// Скачивание файлов (иконки по умолчанию и т.п.). Zero-dependency https.

const http = require('http');
const https = require('https');
const fs = require('fs');

function download(url, dest) {
    const mod = url.startsWith('https:') ? https : http;
    return new Promise((resolve, reject) => {
        const out = fs.createWriteStream(dest);
        const req = mod.get(url, (res) => {
            if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
                out.close();
                req.destroy();
                return download(res.headers.location, dest).then(resolve, reject);
            }
            if (res.statusCode !== 200) {
                out.close();
                fs.rmSync(dest, { force: true });
                return reject(new Error(`HTTP ${res.statusCode} downloading ${url}`));
            }
            res.pipe(out);
            out.on('finish', () => {
                out.close(() => resolve(dest));
            });
        });
        req.on('error', (e) => {
            out.close();
            fs.rmSync(dest, { force: true });
            reject(e);
        });
    });
}

module.exports = { download };