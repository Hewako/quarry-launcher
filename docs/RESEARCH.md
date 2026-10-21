# Сверка внешних API (21.10.2026)

Что проверено и откуда. «Данные» — проверено запросом к самому API, «док.» — по документации.
Всё, что не попало сюда, в коде помечено «не подтверждено первоисточником».

## Mojang

| Факт | Источник |
|---|---|
| Манифест `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`: поля `id, type, url, time, releaseTime, sha1, complianceLevel`; 920 версий, последняя 26.3 | данные |
| `javaVersion` есть у 26.3 (25, `java-runtime-epsilon`), 1.21.11 (21), 1.20.1 (17), 1.16.5 / 1.12.2 / 1.8.9 (8, `jre-legacy`). **Нет у 1.6.4** | данные |
| `logging.client` (`argument: -Dlog4j.configurationFile=${path}`, `file{id,sha1,size,url}`) есть у 1.7+ (1.8.9, 1.12.2, 1.16.5 → `client-1.12.xml`; 26.3 → `client-1.21.2.xml`); нет у 1.6.4 и раньше (там нет log4j) | данные |
| 1.16.5 и 1.12.2 содержат `log4j-core 2.8.1` (уязвимая к CVE-2021-44228) — защищает только logging-конфиг из JSON | данные |
| `arguments.game/jvm` с `rules` (`os.name/arch/version`, `features`: `is_demo_user`, `has_custom_resolution`, `has_quick_plays_support`, `is_quick_play_singleplayer/multiplayer/realms`) — 1.13+; до этого `minecraftArguments` (строка) | данные |
| 26.x: нативы — отдельные библиотеки с классификатором в имени и правилом по ОС, лежат в classpath; JVM-аргументы задают подкаталоги `${natives_directory}/java, /jna, /lwjgl, /netty` | данные |
| ≤1.18: нативы через `natives{os→classifier}` + `downloads.classifiers` + `extract.exclude`; правило `os.version` (регулярное выражение) у 1.6.4 и 1.16.5 | данные |
| Ассеты: `assets: legacy` (1.6.x, виртуальная раскладка) и `pre-1.6` (ресурсы в `gameDir/resources`) | данные |

## Java (Adoptium)

| Факт | Источник |
|---|---|
| `GET https://api.adoptium.net/v3/assets/latest/{major}/hotspot?os=&architecture=&image_type=jre&vendor=eclipse` → `binary.package.{link, checksum (sha256), name, size}` | данные |
| **Java 8 для macOS aarch64 нет** (пустой ответ); x64 есть → на Apple Silicon Java 8 работает через Rosetta | данные |
| 17, 21, 25 — есть для macOS aarch64 и x64 | данные |

## Загрузчики

| Факт | Источник |
|---|---|
| Fabric: `https://meta.fabricmc.net/v2/versions/loader/{game}` (флаг `stable`), профиль `/versions/loader/{game}/{loader}/profile/json`; у библиотек есть `sha1`, `size`; для 26.3 intermediary `0.0.0` | данные |
| Fabric-профиль 26.3 содержит JVM-аргумент `-DFabricMcEmu= net.minecraft.client.main.Main ` | данные |
| Quilt: `https://meta.quiltmc.org/v3/versions/loader/{game}`, профиль аналогичный; **хэшей у библиотек в профиле нет** → проверка по `.sha1` рядом с файлом в Maven; признака stable нет, бета видна по `-beta` в версии | данные |

## authlib-injector

| Факт | Источник |
|---|---|
| `https://authlib-injector.yushi.moe/artifact/latest.json` → `version, build_number, download_url, checksums.sha256`; закреплена сборка 56 (1.2.8), sha256 `9c7f4343e6c82034958ffb48c14a2cb0c85928be7283103ce17da00c6d5a7b10` | данные |
| Аргумент для Ely.by: `-javaagent:<jar>=ely.by`; лаунчер сам авторизуется и передаёт `accessToken` | док. docs.ely.by/en/authlib-injector.html |

## Ely.by

| Факт | Источник |
|---|---|
| OAuth: тип приложения только **Website**; обмен кода — `client_id`, **`client_secret`**, `redirect_uri`, `grant_type`, `code`. **PKCE и клиенты без секрета не упоминаются**; про loopback redirect ничего не сказано | док. docs.ely.by/en/oauth.html |
| Scopes: `account_info`, `account_email`, `offline_access`, `minecraft_server_session`; `expires_in` 86400; refresh token «не истекает»; обновление требует `client_secret` | док. |
| Для лаунчеров есть Yggdrasil-совместимый `https://authserver.ely.by/auth/{authenticate,refresh,validate,invalidate}`; документация рекомендует OAuth | док. docs.ely.by/en/minecraft-auth.html |
| Скины: `http://skinsystem.ely.by/skins/{nick}.png`, `/textures/{nick}`; HTTPS в документации не упомянут | док. docs.ely.by/en/skins-system.html |

## Microsoft / Minecraft Services

| Факт | Источник |
|---|---|
| Секрет клиента не нужен; тенант **`consumers`** обязателен для `XboxLive.signin` | док. minecraft.wiki/w/Microsoft_authentication |
| Device code: `POST https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode` (`client_id`, `scope`), ответ `device_code, user_code, verification_uri, expires_in, interval, message`; опрос `POST …/oauth2/v2.0/token` с `grant_type=urn:ietf:params:oauth:grant-type:device_code`; ошибки `authorization_pending`, `authorization_declined`, `bad_verification_code`, `expired_token` (`slow_down` на странице не описан — обрабатываем по RFC 8628, не подтверждено страницей Microsoft) | док. learn.microsoft.com v2-oauth2-device-code |
| Refresh для публичного клиента: `client_id, scope, refresh_token, grant_type=refresh_token`, без секрета; старый refresh token заменять новым | док. learn.microsoft.com v2-oauth2-auth-code-flow |
| Xbox: `POST https://user.auth.xboxlive.com/user/authenticate` (`RpsTicket: "d=<token>"`, `RelyingParty: http://auth.xboxlive.com`) → `Token`, `DisplayClaims.xui[0].uhs` | док. wiki |
| XSTS: `POST https://xsts.auth.xboxlive.com/xsts/authorize` (`SandboxId: RETAIL`, `RelyingParty: rp://api.minecraftservices.com/`); 401 с `XErr`: 2148916227 бан, 2148916233 нет Xbox-профиля, 2148916235 страна, 2148916236/7 подтверждение возраста, 2148916238 детский аккаунт, 2148916262 без описания | док. wiki (неофициальная) |
| `POST https://api.minecraftservices.com/authentication/login_with_xbox` (`identityToken: "XBL3.0 x=<uhs>;<xsts>"`) → `access_token`, `expires_in` 86400; без одобрения приложения — **403** | док. wiki |
| Владение: `GET /entitlements/mcstore` → `items[].name` (`product_minecraft`, `game_minecraft`, Game Pass: `product_game_pass_pc/ultimate`); пустой список — не владеет | док. wiki |
| Профиль: `GET /minecraft/profile` → `id, name, skins, capes`; 404 `NOT_FOUND` — нет профиля (Game Pass без первого входа в официальный лаунчер) | док. wiki |
| Заявка на доступ к API: `https://aka.ms/mce-reviewappid` | док. wiki |
| Страница help.minecraft.net/hc/en-us/articles/16254801392141 | **не прочитана** (таймаут) |
