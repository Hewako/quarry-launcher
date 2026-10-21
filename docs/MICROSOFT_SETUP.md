# Вход через Microsoft: что нужно сделать владельцу проекта

Код входа в Quarry готов: device code → Xbox Live → XSTS → Minecraft Services → проверка покупки.
Чтобы он заработал, нужно **своё** приложение в Microsoft Azure и одобрение Mojang. Это делается
вашей учётной записью Microsoft — лаунчер и Claude сделать это за вас не могут.

Сверено 21.10.2026 по minecraft.wiki/w/Microsoft_authentication и документации Microsoft
(см. `docs/RESEARCH.md`). Названия пунктов в портале Azure могут отличаться — ориентируйтесь по смыслу.

## 1. Регистрация приложения в Azure (≈10 минут)

1. Откройте https://portal.azure.com → **Microsoft Entra ID** → **App registrations** → **New registration**.
2. **Name**: `Quarry Launcher` (или другое). В названии **нельзя** слова Minecraft, Mojang, Xbox, Microsoft.
3. **Supported account types**: *Personal Microsoft accounts only* (только личные учётные записи).
   Для `XboxLive.signin` нужен тенант `consumers` — рабочие и учебные аккаунты не подойдут.
4. **Redirect URI** — не нужен (Quarry использует вход по коду, device code flow).
5. Создайте приложение. На странице **Overview** скопируйте **Application (client) ID**.
6. **Authentication** → *Advanced settings* → **Allow public client flows** = **Yes** → Save.
   Секрет (client secret) **не создавайте**: в программе на компьютере его нельзя хранить.

Client ID — не секрет: его можно хранить в репозитории и в настройках сборки.

## 2. Заявка Mojang на доступ к Minecraft Services

Новым приложениям Azure нужно разрешение на API Minecraft. Без него вход доходит до XSTS,
а `api.minecraftservices.com` отвечает **403 Invalid app registration** — Quarry покажет это понятным
сообщением.

1. Откройте форму **https://aka.ms/mce-reviewappid**.
2. Укажите Client ID из шага 1, название приложения и ссылку на репозиторий.
3. Ждите ответа. По сообщениям разработчиков — около недели; это не гарантия.

## 3. Сборка Quarry с вашим Client ID

```bash
cd ui && QUARRY_MS_CLIENT_ID=00000000-0000-0000-0000-000000000000 npm run tauri dev
```

или для CLI:

```bash
QUARRY_MS_CLIENT_ID=00000000-0000-0000-0000-000000000000 cargo run -p quarry-core --bin quarry -- account login-microsoft
```

Без переменной кнопка «Войти через Microsoft» неактивна и объясняет, почему.

## Чего делать нельзя

- Брать client ID другого лаунчера (Prism, официального и т. п.) — это нарушает требования Microsoft и Mojang.
- Использовать Ely.by или офлайн-режим как замену Microsoft: Quarry запускает игру только с
  подтверждённой покупкой Java Edition. Для отладки до одобрения есть сборка разработчика
  с `--features dev-launch` (только debug, в релиз не попадает).
