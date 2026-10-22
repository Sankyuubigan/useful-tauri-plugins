/** Общие UI-примитивы панелей image-engine: стиль, эскейп, байты, тосты, прогресс. */
export const STYLE = `
  :host { display: block; color: var(--text, #eee); font-family: var(--font, system-ui, sans-serif); font-size: 13px; }
  .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .row + .row { margin-top: 8px; }
  label { color: var(--text-muted, #999); min-width: 60px; }
  button { padding: 6px 10px; cursor: pointer; border-radius: 6px; border: 1px solid var(--border, #333);
           background: var(--session-hover, #eee); color: var(--text, #eee); font: inherit; font-size: 13px; }
  button.primary { background: var(--primary, #4a90d9); color: #fff; border-color: var(--primary, #4a90d9); }
  button.danger { background: var(--danger, #b54242); color: #fff; border-color: var(--danger, #b54242); }
  button:disabled { opacity: .5; cursor: default; }
  button.tab-btn[aria-pressed="true"] { font-weight: 700; }
  select, input[type=text] { padding: 6px; border-radius: 6px; border: 1px solid var(--border, #333);
           background: var(--bg-elevated, #1c1c1c); color: var(--text, #eee); font: inherit; font-size: 13px; }
  .progress-container { display: none; margin-top: 10px; }
  .progress-status { color: var(--text-muted, #999); font-size: 12px; margin-bottom: 5px; word-break: break-all; }
  .progress-track { height: 8px; border-radius: 6px; background: var(--border, #333); overflow: hidden; }
  .progress-bar { height: 100%; width: 0%; background: var(--primary, #4a90d9); transition: width .1s linear; }
  .hint { color: var(--text-muted, #999); font-size: 12px; white-space: pre-line; }
  .files { margin: 6px 0 0 0; padding-left: 18px; }
  .files li { margin: 2px 0; }
  .ok { color: #6fbf73; }
  .missing { color: var(--text-muted, #999); }
  .overlay { position: fixed; inset: 0; background: rgba(0,0,0,.5); display: none; align-items: center;
             justify-content: center; z-index: 2000; }
  .overlay.open { display: flex; }
  .update-badge { display: inline-block; width: 8px; height: 8px; border-radius: 50%;
                  background: #4caf50; margin-left: 6px; box-shadow: 0 0 6px #4caf50; }
  .box { background: var(--bg-elevated, #1c1c1c); border: 1px solid var(--border, #333); border-radius: 12px;
         padding: 20px; max-width: 520px; width: 90%; }
  .box h3 { margin: 0 0 12px; font-size: 16px; }
  .box p { margin: 8px 0; color: var(--text-muted, #aaa); }
  .overlay-buttons { display: flex; gap: 10px; justify-content: flex-end; margin-top: 16px; }
`;
export function esc(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}
export function formatBytes(n) {
    if (!isFinite(n) || n <= 0)
        return '0 B';
    const units = ['B', 'KB', 'MB', 'GB'];
    let i = 0;
    while (n >= 1024 && i < units.length - 1) {
        n /= 1024;
        i++;
    }
    return `${n.toFixed(1)} ${units[i]}`;
}
/** Лёгкий всплывающий тост (без внешних зависимостей; использует CSS-переменные хоста). */
export function toast(msg, kind = 'success') {
    const el = document.createElement('div');
    el.textContent = msg;
    el.style.cssText =
        `position:fixed; right:16px; bottom:16px; z-index:5000; max-width:420px; padding:10px 14px;` +
            `border-radius:8px; font-size:13px; color:#fff; ` +
            `background:${kind === 'success' ? 'rgba(60,140,70,.95)' : 'rgba(180,60,60,.95)'};`;
    document.body.appendChild(el);
    setTimeout(() => el.remove(), 4200);
}
/** Текст прогресса по байтовому состоянию. */
export function progressText(downloaded, total) {
    const pct = total > 0 ? (downloaded / total) * 100 : 0;
    return `${formatBytes(downloaded)} / ${total > 0 ? formatBytes(total) : '?'}${total > 0 ? ` (${pct.toFixed(0)}%)` : ''}`;
}
