/**
 * Общие стили плашек плагина речи. Темизируются CSS-переменными хоста
 * (PLUGIN_STANDARD §5.5), поэтому живут в Shadow DOM компонентов, а не в глобале.
 *
 * Отдельный модуль, чтобы три панели не держали копию одной и той же строки
 * (core §2.1 SSOT) и чтобы `web-components.ts` не рос вместе с логикой.
 */
export const PANEL_STYLES = `
  :host { display: block; color: var(--text, #333); font-family: var(--font, system-ui, sans-serif); }
  /* Атрибут hidden обязан работать ЛЮБОГО класса с display.
     Правило UA-стилей [hidden] { display: none } проигрывает авторскому
     display из этого файла по происхождению каскада, поэтому .badge.ok
     { display: inline-flex } делал бейдж «обновление доступно» вечно
     видимым, а .dlg-overlay { display: flex } — модалку. Одно правило
     закрывает весь класс ошибок, а не каждый случай по отдельности. */
  [hidden] { display: none !important; }
  * { box-sizing: border-box; }
  button { margin: 4px 4px 0 0; padding: 5px 10px; cursor: pointer; border-radius: 6px;
           border: 1px solid var(--border, #ccc); background: var(--session-hover, #eee);
           color: var(--text, #333); font: inherit; }
  button.primary { background: var(--primary, #89b4fa); color: #1e1e2e; font-weight: 600; border-color: var(--primary, #89b4fa); }
  button.primary:hover:not(:disabled) { background: var(--primary-hover, #74a0f0); }
  button.danger { color: #f38ba8; border-color: #f38ba8; background: transparent; }
  button.danger:hover:not(:disabled) { background: var(--session-hover, #45475a); }
  button:disabled { opacity: .5; cursor: default; }
  select, input, textarea { font: inherit; color: var(--text, #333); background: var(--bg-color, #fff);
           border: 1px solid var(--border, #ccc); border-radius: 6px; padding: 4px 6px; }
  input[readonly] { opacity: .85; }
  input[type="range"] { accent-color: var(--primary, #89b4fa); width: 100%; padding: 0; }
  .muted { color: var(--text-muted, #888); font-size: 12px; }
  .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-top: 6px; }
  .field { margin-top: 8px; }
  .field label { display: block; font-size: 13px; margin-bottom: 2px; }
  .field input[type="text"], .field select { width: 100%; }
  .progress { height: 6px; background: var(--session-hover, #eee); border-radius: 3px; margin-top: 6px; overflow: hidden; }
  .progress > div { height: 100%; background: var(--primary, #4a90d9); width: 0; transition: width .2s; }
  h3 { font-size: 15px; margin: 14px 0 8px; opacity: .85; }
  h4 { font-size: 14px; margin: 12px 0 6px; opacity: .8; }
  hr { border: none; border-top: 1px solid var(--border, #45475a); margin: 16px 0; }
  label { display: block; margin: 10px 0 6px; opacity: .8; }
  .checkbox-row { display: flex; align-items: center; gap: 8px; margin: 12px 0 4px; opacity: .9; }
  .checkbox-row input { flex: none; width: auto; min-width: auto; }
  .hint { opacity: .65; font-size: 13px; margin: 4px 0 10px; color: var(--text, #cdd6f4); }
  .hint.warn { color: #f9e2af; }
  code { background: var(--bg-color, #313244); padding: 1px 6px; border-radius: 4px; word-break: break-all; color: var(--text, #cdd6f4); }
  .vs-head { display: flex; justify-content: space-between; align-items: center; gap: 12px; }
  .vs-head h3 { margin: 14px 0 8px; }
  .voice-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 10px; margin-top: 6px; }
  .voice-card { display: flex; gap: 12px; align-items: center; background: var(--bg-color, #313244);
       border: 1px solid var(--border, #45475a); border-radius: 10px; padding: 10px 12px; }
  .vc-avatar { flex: 0 0 auto; width: 52px; height: 52px; border-radius: 50%; overflow: hidden;
       background: var(--session-hover, #45475a); display: flex; align-items: center; justify-content: center; }
  .vc-avatar img { width: 100%; height: 100%; object-fit: cover; }
  .vc-avatar-ph { font-size: 22px; font-weight: 700; color: var(--text, #cdd6f4); }
  .vc-body { flex: 1 1 auto; min-width: 0; }
  .vc-name { font-size: 14px; font-weight: 600; color: var(--text, #cdd6f4); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .vc-ref { font-size: 12px; opacity: .7; color: var(--text, #cdd6f4); margin-top: 2px; display: -webkit-box;
       -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .vc-date { font-size: 11px; opacity: .5; margin-top: 3px; }
  .vc-actions { flex: 0 0 auto; display: flex; gap: 4px; align-items: center; }
  .vc-actions button { padding: 4px 9px; font-size: 12px; margin: 0; }
  .vc-del { background: transparent; color: #f38ba8; opacity: .8; }
  .vc-del:hover { opacity: 1; background: var(--session-hover, #45475a); }
  .ve-overlay { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.55); display: flex; align-items: center;
       justify-content: center; z-index: 50; padding: 16px; }
  .ve-modal { background: #181825; border: 1px solid var(--border, #45475a); border-radius: 12px; padding: 18px 20px;
       width: 100%; max-width: 440px; max-height: 90vh; overflow-y: auto; box-shadow: 0 20px 50px rgba(0, 0, 0, 0.5);
       color: var(--text, #cdd6f4); }
  .ve-modal h3 { margin: 0 0 12px; font-size: 17px; }
  .ve-modal input[type="text"], .ve-modal textarea { width: 100%; box-sizing: border-box; padding: 8px; border-radius: 6px;
       border: 1px solid var(--border, #45475a); background: var(--bg-color, #313244); color: var(--text, #cdd6f4); }
  .ve-modal textarea { min-height: 64px; resize: vertical; font-family: inherit; }
  .ve-modal input::placeholder, .ve-modal textarea::placeholder { color: var(--text-muted, #7f849c); }
  .status { opacity: .8; font-size: 13px; color: var(--text, #cdd6f4); }
  ul { list-style: none; padding: 0; margin: 8px 0 0; }
  li { display: flex; justify-content: space-between; align-items: center; gap: 8px;
       padding: 6px 10px; background: var(--bg-color, #313244); border-radius: 6px; margin-bottom: 4px; }
  .mname { flex: 1; min-width: 0; font-size: 13px; }
  .badges { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
  button.small { padding: 2px 10px; font-size: 11px; border-radius: 8px; margin: 0; }
  .badge { font-size: 11px; padding: 2px 7px; border-radius: 10px; background: var(--session-hover, #45475a);
           color: var(--text, #cdd6f4); white-space: nowrap; }
  .badge.ru { background: #313244; color: #f38ba8; border: 1px solid #f38ba8; font-weight: 600; }
  .badge.ok { background: #1e2a1e; color: #a6e3a1; display: inline-flex; align-items: center; gap: 5px; }
  .badge.ok::before { content: ''; width: 8px; height: 8px; border-radius: 50%; background: #a6e3a1; box-shadow: 0 0 6px #a6e3a1; }
  .badge.warn { background: #33260f; color: #f9c77a; }

  /* Модалка подтверждения удаления движка (по образцу overlay в llama-плашке). */
  .dlg-overlay { position: fixed; inset: 0; background: rgba(0, 0, 0, .55); display: flex;
                 align-items: center; justify-content: center; z-index: 2000; padding: 16px; }
  .dlg-box { background: var(--bg-elevated, #1c1c1c); border: 1px solid var(--border, #45475a);
             border-radius: 12px; padding: 20px; max-width: 520px; width: 100%; color: var(--text, #cdd6f4); }
  .dlg-box h3 { margin: 0 0 12px; font-size: 17px; }
  .dlg-box p { margin: 8px 0; color: var(--text-muted, #aaa); }
  .dlg-box .strong { color: var(--text, #cdd6f4); word-break: break-all; }
  .dlg-buttons { display: flex; gap: 10px; justify-content: flex-end; margin-top: 16px; }
`;
