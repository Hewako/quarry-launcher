# Quarry

Открытый лаунчер Minecraft: Java Edition для игроков с несколькими профилями и модпаками.
Рабочее название. Не связан с Mojang и Microsoft.

**Состояние:** ранняя разработка (0.1). Что нового — [CHANGELOG.md](CHANGELOG.md),
подробный журнал работы — [docs/PROGRESS.md](docs/PROGRESS.md).
Как описывать изменения — [docs/COMMITS.md](docs/COMMITS.md).

## Что уже работает

- Версии из манифеста Mojang, включая схему 26.x; Java выбирается по `javaVersion` версии и
  показывает причину выбора; нужная Java (Temurin) скачивается и проверяется по SHA-256.
- Изолированные профили: Vanilla, Fabric, Quilt. Совместимость загрузчика проверяется до скачивания.
- Загрузка с проверкой хэшей, повторами, прогрессом и отменой; общий кеш библиотек и ассетов.
- Команда запуска по `arguments` из JSON версии (Quick Play, старые `minecraftArguments`),
  logging-конфиг Mojang против Log4Shell.
- Логи игры с маскированием токенов, домашней папки и IP; итог каждого запуска в профиле.
- Вход Microsoft с проверкой покупки (нужен одобренный client ID — [docs/MICROSOFT_SETUP.md](docs/MICROSOFT_SETUP.md)).
  Ely.by — только вместе с Microsoft-аккаунтом, на котором куплена игра.

## Сборка

Нужны Rust (stable) и Node.js 20+.

```bash
cd ui && npm ci && npm run tauri dev
```

Сборка разработчика, разрешающая запуск без входа (только debug, для проверки на своей копии игры):

```bash
cd ui && npm run tauri dev -- --features dev-launch
```

CLI:

```bash
cargo run -p quarry-core --bin quarry -- --help
```

Тесты: `cargo test --workspace` (сетевые помечены `#[ignore]`: `cargo test -- --ignored`).

## Структура

```
core/          библиотека quarry-core и CLI quarry (без зависимости от интерфейса)
ui/            интерфейс Tauri 2 + React; ui/src-tauri — тонкие команды над ядром
docs/          журнал прогресса, сверка API, инструкции
```

Данные лежат в `~/.quarry` (см. `core/src/paths.rs`); токены — только в хранилище секретов ОС.

## Лицензия

GPL-3.0-or-later, см. [LICENSE](LICENSE).
