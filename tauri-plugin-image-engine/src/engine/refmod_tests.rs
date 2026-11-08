//! Тесты RefMod-карточек (tasks/261104_0230-db2cb59c, Этап 1 Шаг 5 →
//! Этап 2: SSOT формата живёт в плагине, тесты переехали вместе с кодом).
//! Герметично: временные каталоги, синтетические PNG-источники; реальный
//! `<KingOrchData>/refmods` не трогается (все проверки через `_in`-варианты).

use super::*;

fn temp_root(tag: &str) -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "king_orch_refmod_{}_{}_{}",
        tag,
        std::process::id(),
        stamp
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Синтетический PNG-источник заданного размера (декодер `image` его съест).
fn write_source(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let path = dir.join(name);
    image::DynamicImage::new_rgb8(w, h)
        .save_with_format(&path, image::ImageFormat::Png)
        .unwrap();
    path
}

// ── Нормирование габаритов (владелец 08.11.26: бюджет площади как у блоггера —
//    главный кадр ≤2 Мпикс, кратно 32px; прежнее «длинная ≤1024» снято) ──

#[test]
fn resize_caps_area_and_snaps_to_grid() {
    let cases: &[(u32, u32, u32, u32)] = &[
        // 4K-кадр: ужатие по площади до капа главного (2 Мпикс), floor-сетка.
        (4096, 2160, 1920, 1024),
        // Чуть сверх капа: лёгкое ужатие по площади, гарантия ≤ капа.
        (1024, 2048, 992, 1984),
        // В бюджете, но не кратно сетке — выравнивание к ближайшей кратности.
        (830, 1000, 832, 992),
        // Уже в сетке — без изменений.
        (768, 1024, 768, 1024),
        // Микро-кадр — минимальное кратное 32.
        (16, 16, 32, 32),
    ];
    for (w, h, want_w, want_h) in cases {
        let resized = normalize_frame(image::DynamicImage::new_rgb8(*w, *h));
        assert_eq!(
            (resized.width(), resized.height()),
            (*want_w, *want_h),
            "исходник {}x{}",
            w,
            h
        );
        assert!(frame_size_valid(resized.width(), resized.height()));
        assert!(
            resized.width() as u64 * resized.height() as u64 <= CARD_PX_BUDGET_MAIN,
            "кап главного кадра нарушен на {}x{}",
            w,
            h
        );
    }
}

// ── Бюджетная форма подачи (владелец 08.11.26: главный — максимум
//    детальности, остальные по остатку; Σ ≤ 4 Мпикс) ──

#[test]
fn budget_form_main_priority_and_total_cap() {
    // Суммарно в бюджете (8 × 512² = 2.1 Мпикс ≤ 4 Мпикс) — никто не ужат.
    let small: Vec<_> = (0..8)
        .map(|_| image::DynamicImage::new_rgb8(512, 512))
        .collect();
    let out = normalize_frames_budget(&small);
    assert!(
        out.iter().all(|i| (i.width(), i.height()) == (512, 512)),
        "внутри бюджета кадры не трогаются"
    );

    // Превышение: главный (первый) полными пикселями — 1280×768 = 0.98 Мпикс
    // ≤ капа главного 2 Мпикс; остальные делят остаток пропорционально
    // (одинаковые источники → одинаковый share 736×544), Σ ≤ 4 Мпикс.
    let big: Vec<_> = (0..8)
        .map(|_| image::DynamicImage::new_rgb8(1152, 864))
        .collect();
    let mut big = vec![image::DynamicImage::new_rgb8(1280, 768)];
    big.extend((0..7).map(|_| image::DynamicImage::new_rgb8(1152, 864)));
    let out = normalize_frames_budget(&big);
    assert_eq!(
        (out[0].width(), out[0].height()),
        (1280, 768),
        "главный кадр не ужат (влезает в кап 2 Мпикс)"
    );
    for f in &out[1..] {
        assert_eq!(
            (f.width(), f.height()),
            (736, 544),
            "равный share по остатку бюджета"
        );
    }
    let total: u64 = out.iter().map(|i| i.width() as u64 * i.height() as u64).sum();
    assert!(total <= CARD_PX_BUDGET_TOTAL, "бюджет карточки нарушен: {}", total);

    // Главный-гигант: кап главного 2 Мпикс (1632×1216), мелкие не апскейлятся.
    let mut giant = vec![image::DynamicImage::new_rgb8(4000, 3000)];
    giant.extend((0..7).map(|_| image::DynamicImage::new_rgb8(64, 64)));
    let out = normalize_frames_budget(&giant);
    assert_eq!(
        (out[0].width(), out[0].height()),
        (1632, 1216),
        "кап главного 2 Мпикс"
    );
    assert_eq!(
        (out[1].width(), out[1].height()),
        (64, 64),
        "мелкие кадры не апскейлятся"
    );
    let total: u64 = out.iter().map(|i| i.width() as u64 * i.height() as u64).sum();
    assert!(total <= CARD_PX_BUDGET_TOTAL, "бюджет карточки нарушен: {}", total);
}

// ── Slug ──

#[test]
fn slug_validation_matrix() {
    assert!(validate_slug("alice_v2").is_ok());
    assert!(validate_slug("a").is_ok());
    assert!(validate_slug("char-3-4").is_ok());
    // Пустой, слишком длинный, недопустимые символы, traversal, крайние разделители.
    assert!(validate_slug("").is_err());
    assert!(validate_slug("   ").is_err());
    assert!(validate_slug(&"a".repeat(49)).is_err());
    assert!(validate_slug("Alice").is_err());
    assert!(validate_slug("алиса").is_err());
    assert!(validate_slug("../evil").is_err());
    assert!(validate_slug("a/b").is_err());
    assert!(validate_slug("-abc").is_err());
    assert!(validate_slug("abc-").is_err());
}

// ── Save / Load roundtrip ──

#[test]
fn save_load_roundtrip_normalizes_frames() {
    let root = temp_root("roundtrip");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let s1 = write_source(&src_dir, "a.png", 100, 64); // → 96x64 (кратно 32)
    // Σ > бюджета: главный-якорь (a) полными пикселями, b получает остаток
    // (3 993 600 px) → 2720x1440 (floor-сетка, площадь ≤ доли).
    let s2 = write_source(&src_dir, "b.png", 4096, 2160); // → 2720x1440

    let (card, report) = save_refmod_card_in(&root, "test_char", "Тест Чар", "тестовое описание", &[
        s1.clone(),
        s2.clone(),
    ])
    .expect("карточка сохраняется");
    assert_eq!(card.slug, "test_char");
    assert_eq!(card.name, "Тест Чар");
    assert_eq!(card.frames.len(), 2);
    assert!(card.frames[0].ends_with("frame_01.png"));
    assert!(card.frames[1].ends_with("frame_02.png"));

    let loaded = load_refmod_card_in(&root, "test_char").expect("карточка читается");
    assert_eq!(loaded, card);

    // Кадры физически нормированы: 96x64 и 2720x1440.
    let d1 = image::image_dimensions(&loaded.frames[0]).unwrap();
    let d2 = image::image_dimensions(&loaded.frames[1]).unwrap();
    assert_eq!((d1.0, d1.1), (96, 64), "меньше лимита — выравнивание кратно 32");
    assert_eq!(
        (d2.0, d2.1),
        (2720, 1440),
        "4K — остаток бюджета (главный-якорь полными пикселями)"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn save_overwrites_without_stale_frames() {
    let root = temp_root("overwrite");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let sources: Vec<PathBuf> = (0..5)
        .map(|i| write_source(&src_dir, &format!("s{}.png", i), 64, 64))
        .collect();
    save_refmod_card_in(&root, "test_char", "Пять", "", &sources).expect("5 кадров");

    // Перезапись двумя кадрами: frame_03..05 не должны выжить.
    save_refmod_card_in(&root, "test_char", "Два", "", &sources[..2]).expect("перезапись");
    let card = load_refmod_card_in(&root, "test_char").unwrap();
    assert_eq!(card.name, "Два");
    assert_eq!(card.frames.len(), 2, "stale-кадры не должны пережить перезапись");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn save_rejects_bad_inputs() {
    let root = temp_root("rejects");
    let src = write_source(&root, "s.png", 64, 64);
    // >8 кадров.
    let many: Vec<PathBuf> = (0..=FRAMES_MAX)
        .map(|i| write_source(&root, &format!("m{}.png", i), 64, 64))
        .collect();
    let err = save_refmod_card_in(&root, "test_char", "N", "", &many).unwrap_err();
    assert!(matches!(err, RefModError::Usage(ref m) if m.contains("максимум")), "{:?}", err);
    // Пустой список источников.
    let err = save_refmod_card_in(&root, "test_char", "N", "", &[]).unwrap_err();
    assert!(matches!(err, RefModError::Usage(_)), "{:?}", err);
    // Пустое имя.
    let err = save_refmod_card_in(&root, "test_char", "  ", "", &[src.clone()]).unwrap_err();
    assert!(matches!(err, RefModError::Usage(ref m) if m.contains("name")), "{:?}", err);
    // Плохой slug.
    let err = save_refmod_card_in(&root, "Bad Slug", "N", "", &[src]).unwrap_err();
    assert!(matches!(err, RefModError::Usage(ref m) if m.contains("slug")), "{:?}", err);
    // Нечитаемый источник.
    let broken = root.join("broken.png");
    std::fs::write(&broken, b"not a png").unwrap();
    let err = save_refmod_card_in(&root, "test_char", "N", "", &[broken]).unwrap_err();
    assert!(matches!(err, RefModError::Io(ref m) if m.contains("кадр-источник")), "{:?}", err);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn load_missing_card_is_not_found() {
    let root = temp_root("missing");
    let err = load_refmod_card_in(&root, "nobody").unwrap_err();
    assert!(matches!(err, RefModError::NotFound(ref m) if m.contains("nobody")), "{:?}", err);
    // Slug-гейт срабатывает раньше диска.
    let err = load_refmod_card_in(&root, "../evil").unwrap_err();
    assert!(matches!(err, RefModError::Usage(_)), "{:?}", err);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn load_rejects_off_grid_and_oversized_cards() {
    let root = temp_root("corrupt");
    let src = write_source(&root, "s.png", 64, 64);
    // Карточка с кадром, стороны которого НЕ кратны 32 (подмена вручную).
    let card = save_refmod_card_in(&root, "test_char", "N", "", &[src]).unwrap().0;
    let frame = &card.frames[0];
    image::DynamicImage::new_rgb8(50, 50)
        .save_with_format(frame, image::ImageFormat::Png)
        .unwrap();
    let err = load_refmod_card_in(&root, "test_char").unwrap_err();
    assert!(
        matches!(err, RefModError::Usage(ref m) if m.contains("кратны")),
        "несеточный кадр отклоняется: {:?}",
        err
    );

    // Карточка с 9 кадрами (больше лимита хранения) — честный отказ.
    let frames_dir = root.join("test_char").join("frames");
    std::fs::create_dir_all(&frames_dir).unwrap();
    for i in 0..9 {
        image::DynamicImage::new_rgb8(32, 32)
            .save_with_format(frames_dir.join(format!("frame_{:02}.png", i + 1)), image::ImageFormat::Png)
            .unwrap();
    }
    let err = load_refmod_card_in(&root, "test_char").unwrap_err();
    assert!(
        matches!(err, RefModError::Usage(ref m) if m.contains("повреждена")),
        "{:?}",
        err
    );
    let _ = std::fs::remove_dir_all(root);
}

// ── Каталог и prompt-блок ──

#[test]
fn list_and_prompt_cover_cards_and_skip_broken() {
    let root = temp_root("catalog");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let s1 = write_source(&src_dir, "a.png", 64, 64);
    let s2 = write_source(&src_dir, "b.png", 64, 64);
    save_refmod_card_in(&root, "beta_char", "Бета", "второй", &[s2]).unwrap();
    save_refmod_card_in(&root, "alpha_char", "Альфа", "первый", &[s1]).unwrap();
    // Битая папка без meta.json и файл-не-папка — в каталог не попадают.
    std::fs::create_dir_all(root.join("no_meta")).unwrap();
    std::fs::write(root.join("loose.txt"), b"x").unwrap();

    let cards = list_refmod_cards_in(&root);
    let slugs: Vec<&str> = cards.iter().map(|c| c.slug.as_str()).collect();
    assert_eq!(slugs, vec!["alpha_char", "beta_char"], "сортировка по slug, битое пропущено");

    let prompt = refmods_prompt_in(&root);
    assert!(prompt.contains("alpha_char — Альфа — первый (кадров: 1)"), "{}", prompt);
    assert!(prompt.contains("beta_char"), "{}", prompt);

    // Пустой каталог — честная подсказка тула.
    let empty = temp_root("empty_catalog");
    let hint = refmods_prompt_in(&empty);
    assert!(hint.contains("RefMod-карточек персонажей нет"), "{}", hint);
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(empty);
}

// ── Отбор кадров для прогона (владелец 05.11.26: ВСЕ кадры карточки — референсы) ──

#[test]
fn engine_ref_paths_takes_all_frames() {
    let root = temp_root("take_all");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let sources: Vec<PathBuf> = (0..5)
        .map(|i| write_source(&src_dir, &format!("s{}.png", i), 64, 64))
        .collect();
    let (card, _) = save_refmod_card_in(&root, "test_char", "Пять", "", &sources).unwrap();
    assert_eq!(card.frames.len(), 5);
    let paths = engine_ref_paths(&card);
    assert_eq!(paths.len(), 5, "все 5 кадров идут в движок, не первые 3");
    for (i, p) in paths.iter().enumerate() {
        assert_eq!(p, &card.frames[i].to_string_lossy(), "порядок сохранён");
    }
    // Потолок формата: карточка на FRAMES_MAX кадров — тоже целиком.
    let full: Vec<PathBuf> = (0..FRAMES_MAX)
        .map(|i| write_source(&src_dir, &format!("f{}.png", i), 64, 64))
        .collect();
    let card8 = save_refmod_card_in(&root, "test_char8", "Восемь", "", &full).unwrap().0;
    assert_eq!(engine_ref_paths(&card8).len(), FRAMES_MAX);
    let _ = std::fs::remove_dir_all(root);
}

// ── Отчёт нормализации (PRP 07.11.26: сжатие кадров карточки — видимым) ──

#[test]
fn normalize_report_tracks_resized_frames() {
    let root = temp_root("report");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    // Оверсайз: площадь > капа главного → ужатие по бюджету площади.
    let s1 = write_source(&src_dir, "big.png", 1024, 2048); // → 992x1984
    // Уже в сетке: в отчёт не попадает.
    let s2 = write_source(&src_dir, "grid.png", 320, 288); // → 320x288
    // Мелкий невыровненный: растяжение к сетке 32 (площадь растёт).
    let s3 = write_source(&src_dir, "off_grid.png", 305, 528); // → 320x544
    let (_, report) = save_refmod_card_in(&root, "test_char", "Отчёт", "", &[s1, s2, s3])
        .expect("карточка сохраняется");
    assert_eq!(
        report.resized,
        vec![
            FrameResize {
                frame_file: "frame_01.png".into(),
                src_w: 1024,
                src_h: 2048,
                dst_w: 992,
                dst_h: 1984,
            },
            FrameResize {
                frame_file: "frame_03.png".into(),
                src_w: 305,
                src_h: 528,
                dst_w: 320,
                dst_h: 544,
            },
        ],
        "сеточный кадр в отчёт не попадает, oversize → сжатие, невыровненный → растяжение"
    );
    let _ = std::fs::remove_dir_all(root);
}

// ── Миниатюра для плашки «Рефмодс» (Этап 2, Шаг 4) ──

#[test]
fn thumbnail_is_bounded_jpeg_data_url() {
    let root = temp_root("thumb");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    // 1024x512 → длинная сторона миниатюры ровно 256.
    let s = write_source(&src_dir, "face.png", 1024, 512);
    save_refmod_card_in(&root, "thumb_char", "Мини", "", &[s]).unwrap();

    let url = thumbnail_data_url_in(&root, "thumb_char").expect("миниатюра строится");
    let (meta, b64) = url
        .split_once(",")
        .expect("data URL с запятой");
    assert_eq!(meta, "data:image/jpeg;base64", "{}", meta);
    assert!(!b64.is_empty());

    // Декодируем обратно и проверяем габарит ≤256 по длинной стороне.
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("base64 валиден");
    let decoded = image::load_from_memory(&bytes).expect("JPEG декодируется");
    assert_eq!(decoded.width().max(decoded.height()), THUMBNAIL_LONG_SIDE);

    // Несуществующая карточка — NotFound.
    let err = thumbnail_data_url_in(&root, "nobody").unwrap_err();
    assert!(matches!(err, RefModError::NotFound(_)), "{:?}", err);
    let _ = std::fs::remove_dir_all(root);
}
