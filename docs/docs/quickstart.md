# Быстрый старт

Эта страница проведёт вас от клонирования репозитория до первого результата поиска. Самый короткий путь для знакомства - Docker: образ собирается локально из `Dockerfile`, внутри уже есть веб-интерфейс, админ-панель, документация и шаблон конфигурации.

:::note[Примечание]
Нужен только Docker (для варианта с Compose - плагин `docker compose`). Rust и Node.js ставить не требуется: всё собирается внутри образа. Для постоянной установки на Linux-сервер удобнее установщик `scripts/install.sh` - см. [Установка](installation.md).
:::

## 1. Получите исходники

```bash
git clone https://github.com/sheinices/crabindex.git
cd crabindex
```

## 2. Соберите и запустите контейнер

Минимальный вариант - один контейнер с двумя томами:

```bash
docker build -t crabindex .
docker run -d --name crabindex --restart unless-stopped \
  -p 9117:9117 \
  -v crabindex-config:/app/config \
  -v crabindex-data:/app/Data \
  crabindex
```

Или через Compose. В репозитории есть готовый [docker-compose.example.yml](https://github.com/sheinices/crabindex/blob/main/docker-compose.example.yml) - вместе с FlareSolverr, WARP и cffetch для трекеров за Cloudflare. Для первого знакомства достаточно короткого файла в корне репозитория:

```yaml
# docker-compose.yml
services:
  crabindex:
    image: crabindex:latest
    build:
      context: .
    container_name: crabindex
    restart: unless-stopped
    ports:
      - "9117:9117"
    volumes:
      - ./config:/app/config
      - ./data:/app/Data
    environment:
      - TZ=Europe/Moscow
```

```bash
docker compose up -d --build
```

При первом старте entrypoint не найдёт конфигурацию и скопирует шаблон `Data/example.yaml` в `/app/config/init.yaml`. Затем сервер сгенерирует токен входа в админ-панель (`admin.token`) и пароль (`devkey`) и допишет их в этот файл. Дальше меняйте настройки в админ-панели или правьте именно этот файл (в примере выше - `./config/init.yaml`): сервер перечитывает его сам, перезапуск не нужен.

:::note[Примечание]
Контейнер работает от пользователя с UID/GID `1000:1000`. Если вы монтируете каталоги хоста (`./config`, `./data`), они должны быть доступны этому пользователю на запись.
:::

## 3. Проверьте, что сервер работает

```bash
curl -s http://localhost:9117/health
# {"status":"OK"}

curl -s http://localhost:9117/version
```

## 4. Откройте веб-интерфейс

Перейдите на `http://localhost:9117/`:

| Страница | Что показывает |
| --- | --- |
| `/` | Поиск по FileDB |
| `/stats` | Статистика по трекерам |
| `/swagger` | Интерактивная документация API |
| `/docs/` | Эта документация |

## 5. Войдите в админ-панель

Настройки, фоновые задачи, ручной запуск парсеров, обслуживание базы и логи находятся в отдельной [админ-панели](admin.md). Её адрес с секретным токеном и пароль выводятся в лог при первом запуске:

```bash
docker logs crabindex 2>&1 | grep 'admin:'
# admin: http://<host>:9117/admin?Z0mt0N7r2hoUM2TuOk
# admin: devkey: 9fQ2…
```

Замените `<host>` на адрес машины (например, `localhost`), откройте ссылку и введите `devkey`. Повторно показать адрес и ключ можно командой `docker exec crabindex ./crabindex admin`.

:::tip[Совет]
Чтобы панель сразу получила свой путь вместо `/admin`, до первого запуска задайте переменную окружения `CRABINDEX_ADMIN_PATH=/my-panel` (`-e` у `docker run` или `environment:` в Compose).
:::

## 6. Дождитесь наполнения базы

Сразу после установки FileDB пуста. Шаблон `Data/example.yaml` уже содержит `syncapi: https://sync.crab.rip`, поэтому встроенный sync-воркер примерно через 20 секунд после старта начинает загружать готовую базу. Первая загрузка большой базы занимает заметное время; прогресс виден в логах:

```bash
docker logs -f crabindex
```

Если вы хотите парсить трекеры самостоятельно, установите расписание из `Data/crontab` на хосте - см. [Cron](deployment/cron.md) и [Синхронизация](concepts/sync.md).

## 7. Выполните первый поиск

```bash
# Текстовый поиск (формат Jackett)
curl -s "http://localhost:9117/api/v2.0/indexers/all/results?query=матрица"

# Поиск по IMDb ID
curl -s "http://localhost:9117/api/v2.0/indexers/all/results?query=tt0133093"

# Поиск по Кинопоиску
curl -s "http://localhost:9117/api/v2.0/indexers/all/results?query=kp301"

# Собственный JSON API
curl -s "http://localhost:9117/api/v1.0/torrents?search=матрица"
```

## 8. Подключите клиента

| Клиент | Адрес | API-ключ |
| --- | --- | --- |
| Prisma (Jackett) | `http://ВАШ_ХОСТ:9117` | значение `apikey` или пусто |
| Lampa (Jackett) | `http://ВАШ_ХОСТ:9117` | значение `apikey` или пусто |
| Sonarr / Radarr (Torznab) | `http://ВАШ_ХОСТ:9117/torznab/api` | значение `apikey` |
| Prowlarr (Generic Torznab) | `http://ВАШ_ХОСТ:9117/api/v1/indexer/1/newznab` | значение `apikey` |

Подробности - на странице [Клиенты](clients/overview.md).

:::warning[Внимание]
Пока `apikey` пуст, поиск открыт всем, кто может достучаться до порта 9117. Если сервер виден из интернета, задайте `apikey` и публикуйте сервер через обратный прокси с HTTPS - см. [Аутентификация](authentication.md) и [Админ-панель](admin.md).
:::

## Дальше

- [Админ-панель](admin.md) - вход, смена пути, токена и ключа.
- [Конфигурация](configuration/overview.md) - `apikey`, трекеры, FlareSolverr.
- [Docker](deployment/docker.md) - полный стек с FlareSolverr, WARP и cffetch.
- [Трекеры](trackers/overview.mdx) - авторизация на каждом трекере.
- [Cron](deployment/cron.md) - расписание парсинга.
