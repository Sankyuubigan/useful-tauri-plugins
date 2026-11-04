/**
 * <image-refmod-panel> — плашка «Рефмодс» (tasks/261104_0230-db2cb59c, Этап 2):
 * список RefMod-карточек персонажей (`<data>/refmods/<slug>/`) с миниатюрами.
 * Карточки создаются агентом в чате (тул refmod_create), плашка — только чтение:
 * refresh + список + миниатюры + пустое состояние.
 */
import { STYLE, esc } from './ui-shared';
import { listRefmodCards, refmodCardThumbnail } from '../index';
export class RefmodPanel extends HTMLElement {
    connectedCallback() {
        if (this.root)
            return;
        this.root = this.attachShadow({ mode: 'open' });
        this.root.innerHTML = `
      <style>${STYLE}
        .cards { display: flex; flex-direction: column; gap: 8px; margin-top: 8px; }
        .card { display: flex; gap: 10px; align-items: center; border: 1px solid var(--border, #333);
                border-radius: 8px; padding: 8px 10px; }
        .thumb { width: 64px; height: 64px; border-radius: 6px; object-fit: cover;
                 background: var(--bg-elevated, #1c1c1c); flex: none; }
        .meta { min-width: 0; }
        .slug { font-weight: 700; word-break: break-all; }
        .desc { color: var(--text-muted, #999); font-size: 12px; word-break: break-word; }
        .frames { color: var(--text-muted, #999); font-size: 12px; }
        .empty { color: var(--text-muted, #999); margin-top: 8px; white-space: pre-line; }
      </style>
      <div>
        <div class="row">
          <button id="refresh" class="primary">Обновить</button>
          <span class="hint">Персонажи для генерации видео. Создаются агентом в чате:
            прикрепи фото персонажа и попроси создать персонажа.</span>
        </div>
        <div class="cards" id="cards"></div>
        <div class="empty" id="empty" style="display:none;">RefMod-карточек персонажей нет.

Прикрепи в чат 1–8 фото одного персонажа (любых размеров — агент сам нормирует) и попроси создать персонажа.</div>
      </div>`;
        this.root.getElementById('refresh')?.addEventListener('click', () => this.refresh());
        this.refresh();
    }
    async refresh() {
        const cardsBox = this.root.getElementById('cards');
        const emptyBox = this.root.getElementById('empty');
        if (!cardsBox || !emptyBox)
            return;
        cardsBox.innerHTML = '';
        emptyBox.style.display = 'none';
        let cards = [];
        try {
            cards = await listRefmodCards();
        }
        catch (e) {
            emptyBox.textContent = `Не удалось получить список карточек: ${String(e)}`;
            emptyBox.style.display = 'block';
            return;
        }
        if (cards.length === 0) {
            emptyBox.style.display = 'block';
            return;
        }
        for (const card of cards) {
            const row = document.createElement('div');
            row.className = 'card';
            const img = document.createElement('img');
            img.className = 'thumb';
            img.alt = `Миниатюра ${card.slug}`;
            refmodCardThumbnail(card.slug)
                .then((url) => { img.src = url; })
                .catch(() => { img.alt = 'нет миниатюры'; });
            const meta = document.createElement('div');
            meta.className = 'meta';
            const title = card.name && card.name !== card.slug ? `${card.slug} — ${card.name}` : card.slug;
            meta.innerHTML = `
        <div class="slug">${esc(title)}</div>
        ${card.description ? `<div class="desc">${esc(card.description)}</div>` : ''}
        <div class="frames">Кадров: ${card.frames}</div>`;
            row.appendChild(img);
            row.appendChild(meta);
            cardsBox.appendChild(row);
        }
    }
}
if (!customElements.get('image-refmod-panel')) {
    customElements.define('image-refmod-panel', RefmodPanel);
}
