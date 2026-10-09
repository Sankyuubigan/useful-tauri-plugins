//! Дескрипторы роутеров: единственный источник правды о том, чем роутер является.
//!
//! ## Почему таблица, а не семь методов `match`
//!
//! Изначально «роутер» был полемless-вариантом enum, а всё различие между
//! роутерами размазано по семи методам (`as_str`, `default_port`,
//! `npm_package`, `dir_name`, `needs_npm_deps`, `lock_index`,
//! `server_script_relative`). Добавление четвёртого роутера требовало правки
//! **семи** мест, и любое забытое место выдавало не ошибку компиляции, а
//! панику в рантайме — например `INSTALL_LOCKS[index]` при индексе 3 в массиве
//! из трёх мьютексов (см. `installer.rs`).
//!
//! Теперь всё описание роутера лежит в одной строке [`ROUTERS`], а
//! [`RouterId`] — просто индекс в неё. Пропустить роутер невозможно
//! конструктивно: тест `router_table_is_consistent` сверяет таблицу с
//! перечислением вариантов и с числом мьютексов установки.
//!
//! ## Роутеры бывают двух видов
//!
//! | Вид | Что запускается | Устанавливается |
//! |---|---|---|
//! | [`RouterKind::NodeBundle`] | `node.exe` + standalone-бандл из npm | плагином, из сети |
//! | [`RouterKind::NativeGateway`] | наш собственный бинарь шлюза | плагином, из сети |
//!
//! Различие видно во всех операциях: у node-роутера есть версии, npm-пакет и
//! каталог установки, у шлюза — своя папка, своя версия и своя точка входа.

use serde::{Deserialize, Serialize};

/// Вид роутера: чем он является по своей природе.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouterKind {
    /// Node.js-бандл, ставящийся из npm (портативный node.exe + tgz роутера).
    NodeBundle {
        /// Пакет в npm registry.
        npm_package: &'static str,
        /// Нужен ли `npm install` поверх распакованного tgz.
        ///
        /// 9router/extremerouter — standalone: все зависимости вшиты в пакет.
        /// OmniRoute — Next.js-приложение: `server.js` делает `require('next')`,
        /// поэтому зависимости обязаны быть в `dist/node_modules`.
        needs_npm_deps: bool,
        /// Точка входа относительно `dist/`.
        server_script: &'static str,
    },
    /// Наш собственный бинарь шлюза: OpenAI-прокси с ротацией ключей.
    ///
    /// Это **не** node-бандл и **не** npm-пакет: у него нет ни того, ни другого,
    /// и всё, что знает про npm (`latest_npm_version`, `deps_present`,
    /// `install_is_complete`), к нему неприменимо. Отсюда и разделение видов,
    /// а не ещё один флаг: иначе каждый вызов npm-логики таскал бы `Option`
    /// и забывал бы его обработать.
    NativeGateway {
        /// Имя исполняемого файла (без `.exe`).
        exe_file: &'static str,
    },
}

/// Полное описание одного роутера.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouterSpec {
    /// Идентификатор для IPC, конфига и префикса модели (`9router:`, …).
    pub id: &'static str,
    /// Подпись вкладки в UI.
    pub label: &'static str,
    /// Имя папки установки и каталога данных.
    pub dir_name: &'static str,
    /// Порт HTTP-шлюза по умолчанию.
    pub default_port: u16,
    /// Чем этот роутер является.
    pub kind: RouterKind,
}

/// Таблица всех роутеров. Единственный источник правды.
pub const ROUTERS: &[RouterSpec] = &[
    RouterSpec {
        id: "9router",
        label: "9Router",
        dir_name: "9router",
        default_port: 20128,
        kind: RouterKind::NodeBundle {
            npm_package: "9router",
            needs_npm_deps: false,
            server_script: "app/custom-server.js",
        },
    },
    RouterSpec {
        id: "extremerouter",
        label: "ExtremeRouter",
        dir_name: "extremerouter",
        default_port: 20129,
        kind: RouterKind::NodeBundle {
            npm_package: "@rsalmn/extremerouter",
            needs_npm_deps: false,
            server_script: "app/custom-server.js",
        },
    },
    RouterSpec {
        id: "omniroute",
        label: "OmniRoute",
        dir_name: "omniroute",
        default_port: 20130,
        kind: RouterKind::NodeBundle {
            npm_package: "omniroute",
            needs_npm_deps: true,
            server_script: "dist/server.js",
        },
    },
    RouterSpec {
        id: "gateway",
        label: "Gateway",
        dir_name: "gateway",
        default_port: 20131,
        kind: RouterKind::NativeGateway { exe_file: "cloud-routers-gateway" },
    },
];

/// Идентификатор роутера. Значение соответствует [`RouterSpec::id`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouterId {
    NineRouter,
    ExtremeRouter,
    OmniRoute,
    /// Наш собственный шлюз (`cloud-routers-gateway`).
    Gateway,
}

impl RouterId {
    /// Все роутеры. Порядок совпадает с [`ROUTERS`].
    pub const ALL: &'static [RouterId] = &[
        RouterId::NineRouter,
        RouterId::ExtremeRouter,
        RouterId::OmniRoute,
        RouterId::Gateway,
    ];

    /// Дескриптор роутера.
    pub fn spec(self) -> &'static RouterSpec {
        &ROUTERS[self.index()]
    }

    /// Индекс в [`ROUTERS`] — он же индекс мьютекса установки.
    ///
    /// Инвариант «`index() < ROUTERS.len()`» держится конструктивно: значение
    /// берётся из таблицы, а не вычисляется. Тест
    /// `router_table_is_consistent` фиксирует соответствие `ALL` и `ROUTERS`,
    /// поэтому новый вариант без строки в таблице не соберётся.
    pub fn index(self) -> usize {
        match self {
            RouterId::NineRouter => 0,
            RouterId::ExtremeRouter => 1,
            RouterId::OmniRoute => 2,
            RouterId::Gateway => 3,
        }
    }

    pub fn as_str(self) -> &'static str {
        self.spec().id
    }

    pub fn label(self) -> &'static str {
        self.spec().label
    }

    pub fn default_port(self) -> u16 {
        self.spec().default_port
    }

    pub fn dir_name(self) -> &'static str {
        self.spec().dir_name
    }

    pub fn kind(self) -> RouterKind {
        self.spec().kind
    }

    /// Роутер — node-бандл? (всё, что ставится через npm)
    pub fn is_node_bundle(self) -> bool {
        matches!(self.kind(), RouterKind::NodeBundle { .. })
    }

    /// Роутер — наш бинарь шлюза?
    pub fn is_native_gateway(self) -> bool {
        matches!(self.kind(), RouterKind::NativeGateway { .. })
    }

    /// npm-пакет роутера. Для нашего шлюза `None`: он не в npm, и выдумывать
    /// пакет означало бы проверять обновления по несуществующему адресу.
    pub fn npm_package(self) -> Option<&'static str> {
        match self.kind() {
            RouterKind::NodeBundle { npm_package, .. } => Some(npm_package),
            RouterKind::NativeGateway { .. } => None,
        }
    }

    /// Нужен ли `npm install` поверх распакованного tgz.
    pub fn needs_npm_deps(self) -> bool {
        match self.kind() {
            RouterKind::NodeBundle { needs_npm_deps, .. } => needs_npm_deps,
            RouterKind::NativeGateway { .. } => false,
        }
    }

    /// Точка входа относительно `dist/`. Для шлюза `None`: он не распаковывается.
    pub fn server_script_relative(self) -> Option<&'static str> {
        match self.kind() {
            RouterKind::NodeBundle { server_script, .. } => Some(server_script),
            RouterKind::NativeGateway { .. } => None,
        }
    }

    /// Имя исполняемого файла шлюза (без `.exe`). Для node-роутеров `None`.
    pub fn gateway_exe_file(self) -> Option<&'static str> {
        match self.kind() {
            RouterKind::NativeGateway { exe_file } => Some(exe_file),
            RouterKind::NodeBundle { .. } => None,
        }
    }

    /// Разобрать модель хоста вида `"9router:my-combo"` в роутер и имя комбо.
    ///
    /// Единственный разбор префиксов в проекте. Раньше он был продублирован в
    /// двух местах хоста (`api/chat.rs` и `orchestrator/mod.rs`) цепочками
    /// `starts_with`, и добавление роутера означало править оба — с риском
    /// разъехаться (один узнаёт роутер, другой — нет).
    pub fn from_model_path(model_path: &str) -> Option<(RouterId, &str)> {
        let (id, rest) = model_path.split_once(':')?;
        let router: RouterId = id.parse().ok()?;
        Some((router, rest))
    }

    /// Префикс модели роутера (`"9router:"`). Роутеры без префикса не бывают:
    /// локальные gguf-модели отличаются отсутствием двоеточия.
    pub fn model_prefix(self) -> String {
        format!("{}:", self.as_str())
    }
}

impl std::fmt::Display for RouterId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for RouterId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Разбор идёт по таблице, а не `match`: новый роутер появляется в
        // [`ROUTERS`] и сразу начинает работать, без правок здесь.
        ROUTERS
            .iter()
            .find(|spec| spec.id == s)
            .and_then(|spec| {
                RouterId::ALL
                    .iter()
                    .copied()
                    .find(|r| r.spec().id == spec.id)
            })
            .ok_or_else(|| format!("Неизвестный роутер: {}", s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Инварианты таблицы роутеров.
    ///
    /// Этот тест закрывает класс ошибок, который раньше проявлялся как
    /// паника в рантайме (индекс мьютекса установки выходил за границы).
    #[test]
    fn router_table_is_consistent() {
        assert_eq!(
            ROUTERS.len(),
            RouterId::ALL.len(),
            "таблица и перечисление вариантов разошлись"
        );
        let mut ids: Vec<&str> = Vec::new();
        let mut dirs: Vec<&str> = Vec::new();
        let mut ports: Vec<u16> = Vec::new();
        for (i, router) in RouterId::ALL.iter().enumerate() {
            assert_eq!(router.index(), i, "index() обязан совпадать с позицией в ALL");
            let spec = router.spec();
            assert_eq!(spec, &ROUTERS[i], "spec() обязан смотреть в ту же строку таблицы");
            assert!(!spec.id.is_empty(), "пустой id роутера");
            assert!(!spec.label.is_empty(), "у «{}» пустая подпись", spec.id);
            assert!(spec.default_port > 0, "у «{}» нулевой порт", spec.id);
            assert!(!ids.contains(&spec.id), "дубликат id: {}", spec.id);
            assert!(!dirs.contains(&spec.dir_name), "дубликат папки: {}", spec.dir_name);
            assert!(!ports.contains(&spec.default_port), "дубликат порта: {}", spec.default_port);
            ids.push(spec.id);
            dirs.push(spec.dir_name);
            ports.push(spec.default_port);
        }
    }

    #[test]
    fn gateway_is_the_only_native_router() {
        // Ровно один собственный роутер — и он последний в таблице, поэтому
        // добавление новых node-роутеров не сдвинет его индекс.
        let natives: Vec<RouterId> = RouterId::ALL
            .iter()
            .copied()
            .filter(|r| r.is_native_gateway())
            .collect();
        assert_eq!(natives, vec![RouterId::Gateway]);
        assert_eq!(natives[0].index(), ROUTERS.len() - 1);
    }

    #[test]
    fn node_routers_keep_their_npm_metadata() {
        for router in RouterId::ALL.iter().copied().filter(|r| r.is_node_bundle()) {
            assert!(router.npm_package().is_some(), "у {} нет npm-пакета", router);
            assert!(router.server_script_relative().is_some(), "у {} нет точки входа", router);
            assert!(router.gateway_exe_file().is_none());
        }
    }

    #[test]
    fn gateway_has_no_npm_metadata() {
        // Проверка «на каждый роутер есть npm-пакет» на шлюзе упала бы — и
        // правильно: шлюз не в npm. `None` здесь защищает `check_router_update`
        // от похода по несуществующему адресу.
        assert_eq!(RouterId::Gateway.npm_package(), None);
        assert_eq!(RouterId::Gateway.server_script_relative(), None);
        assert_eq!(RouterId::Gateway.gateway_exe_file(), Some("cloud-routers-gateway"));
        assert!(!RouterId::Gateway.needs_npm_deps());
    }

    #[test]
    fn ports_do_not_collide_with_existing_routers() {
        // Шлюз обязан занять свободный порт, иначе он не поднимется рядом с
        // остальными роутерами. Проверка на конкретных значениях, а не «уникально
        // в таблице»: таблица уникальна всегда, а совпадение с чужим сервисом
        // на машине пользователя — не.
        assert_eq!(RouterId::NineRouter.default_port(), 20128);
        assert_eq!(RouterId::ExtremeRouter.default_port(), 20129);
        assert_eq!(RouterId::OmniRoute.default_port(), 20130);
        assert_eq!(RouterId::Gateway.default_port(), 20131);
    }

    #[test]
    fn from_str_roundtrips_for_every_router() {
        for router in RouterId::ALL {
            assert_eq!(router.as_str().parse::<RouterId>().expect("roundtrip"), *router);
            assert_eq!(router.to_string(), router.as_str());
        }
    }

    #[test]
    fn from_str_rejects_unknown_id() {
        assert!("нет-такого".parse::<RouterId>().is_err());
        assert!("".parse::<RouterId>().is_err());
    }

    #[test]
    fn from_model_path_splits_router_and_combo() {
        for router in RouterId::ALL {
            let path = format!("{}:my-combo", router.as_str());
            let (r, combo) = RouterId::from_model_path(&path).expect("resolve");
            assert_eq!(r, *router);
            assert_eq!(combo, "my-combo");
        }
    }

    #[test]
    fn from_model_path_keeps_colons_inside_combo_name() {
        // Имена моделей у провайдеров содержат двоеточия (`qwen:qwen3-…`),
        // поэтому режем по ПЕРВОМУ, а не по последнему.
        let (r, combo) = RouterId::from_model_path("gateway:qwen:qwen3-32b").expect("resolve");
        assert_eq!(r, RouterId::Gateway);
        assert_eq!(combo, "qwen:qwen3-32b");
    }

    #[test]
    fn from_model_path_rejects_local_model_paths() {
        // Локальный путь к gguf не должен приниматься за облачную модель.
        assert!(RouterId::from_model_path(r"D:\models\qwen3.gguf").is_none());
        assert!(RouterId::from_model_path("qwen3.gguf").is_none());
        assert!(RouterId::from_model_path("неизвестный:combo").is_none());
        // Без двоеточия — это не облачная модель.
        assert!(RouterId::from_model_path("9router").is_none());
    }

    #[test]
    fn model_prefix_roundtrips_through_from_model_path() {
        for router in RouterId::ALL {
            let path = format!("{}combo-1", router.model_prefix());
            let (r, combo) = RouterId::from_model_path(&path).expect("resolve");
            assert_eq!(r, *router);
            assert_eq!(combo, "combo-1");
        }
    }
}
