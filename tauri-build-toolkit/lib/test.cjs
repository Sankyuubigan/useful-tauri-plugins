// test.bat-Р°РЅР°Р»РѕРі: cargo test (СЃ РѕРїС†РёРѕРЅР°Р»СЊРЅС‹Рј С„РёР»СЊС‚СЂРѕРј) РІ src-tauri.
// РљР°Рє РІ СЌС‚Р°Р»РѕРЅРµ: Сѓ РїР°РєРµС‚Р° РЅРµС‚ lib-С‚Р°СЂРіРµС‚Р°, РїРѕСЌС‚РѕРјСѓ `cargo test` Р±РµР· С„Р»Р°РіР°
// РїРѕРєСЂС‹РІР°РµС‚ РІСЃРµ С‚Р°СЂРіРµС‚С‹ (bin unit-С‚РµСЃС‚С‹). MSVC-РѕРєСЂСѓР¶РµРЅРёРµ РёРЅРёС†РёР°Р»РёР·РёСЂСѓРµС‚ .bat.

const path = require('path');
const { run: sh } = require('./run.cjs');

function run(cfg, extras) {
    const args = ['test', ...(extras || [])];
    // РўРµСЃС‚С‹ СЃ СЂРµР°Р»СЊРЅРѕР№ РјРѕРґРµР»СЊСЋ вЂ” РІСЃРµРіРґР° СЃ #[ignore]; РёС… Р·Р°РїСѓСЃРє С‚РѕР»СЊРєРѕ РїРѕ СЏРІРЅРѕР№
    // РїСЂРѕСЃСЊР±Рµ С‡РµСЂРµР· `cargo test -- --ignored` (РїСЂР°РІРёР»Рѕ РїСЂРѕРµРєС‚Р°).
    sh(cfg, 'cargo', args, { echo: true, cwd: path.join(cfg.projectRoot, 'src-tauri') });
    console.log('\n[test] done.');
}

module.exports = { run };
