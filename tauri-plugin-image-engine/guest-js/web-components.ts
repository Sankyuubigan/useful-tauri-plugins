/**
 * Регистрация Web Components плагина (PRP 21.10.26 Шаг 3: разнесено по
 * файлам-панелям). Вердикт юзера 22.10: движок — отдельная карточка, как у
 * llama-engine; медиа-блок держит только вкладки бандлов.
 *
 * Состав:
 *  - <image-engine-panel> — движок sd.cpp (install/update/remove) — своя карточка;
 *  - <image-media-panel> — вкладки «Картинки»/«Видео», каждая вкладка —
 *    <image-bundle-tab service="image|video"> (по BundleService из services.ts);
 *  - отдельные элементы тоже работают сами по себе.
 *
 * Легаси <image-bundle-panel>/<image-models-panel> УДАЛЕНЫ структурно
 * (кастом-папка теперь внутренность вкладки); хосту достаточно двух карточек.
 */
export { ImageEnginePanel } from './components/engine-panel'
export { BundleTabElement } from './components/bundle-tab'
export { MediaEnginePanel } from './components/media-panel'
