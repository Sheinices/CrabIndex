# Обратный прокси

CrabIndex сам по себе говорит только по HTTP на порту `9117`. Для доступа из интернета поставьте перед ним обратный прокси (Nginx, Caddy, Traefik, cloudflared), который завершает TLS.

:::warning[Внимание]
Запрос через обратный прокси **не** считается запросом из локальной сети, даже если прокси работает на том же хосте или в той же Docker-сети. Заголовки `X-Forwarded-*`, `Forwarded`, `X-Real-IP`, `CF-Connecting-IP`, `CF-Ray` выдают прокси, и для `/cron/*`, `/dev/*` и `/jsondb*` потребуется `devkey`. См. [Матрица доступа](../operations/access-matrix.md).
:::

## 1. Закройте прямой доступ

```yaml
# init.yaml
listenip: 127.0.0.1      # только loopback; применяется после перезапуска
listenport: 9117
apikey: "ключ-для-клиентов"
devkey: "ключ-для-администрирования"   # сгенерирован при первом запуске
```

Если прокси работает в Docker, а CrabIndex на хосте, вместо `127.0.0.1` укажите адрес, доступный прокси, и закройте порт `9117` фаерволом снаружи.

## 2. Настройте прокси

### Nginx

```nginx
server {
    listen 443 ssl http2;
    server_name crabindex.example.com;

    ssl_certificate     /etc/letsencrypt/live/crabindex.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/crabindex.example.com/privkey.pem;

    client_max_body_size 4m;

    location / {
        proxy_pass http://127.0.0.1:9117;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_read_timeout 120s;
    }
}
```

Увеличьте `proxy_read_timeout`, если вызываете длинные задачи `/cron/*` через прокси (например, `rutracker/parse` может идти до часа). Поиск отвечает быстро.

### Caddy

```text
crabindex.example.com {
    reverse_proxy 127.0.0.1:9117
}
```

Caddy сам получает сертификат и передаёт стандартные заголовки `X-Forwarded-*`.

### Cloudflare Tunnel (cloudflared)

```yaml
# config.yml cloudflared
ingress:
  - hostname: crabindex.example.com
    service: http://127.0.0.1:9117
  - service: http_status:404
```

cloudflared передаёт адрес посетителя в `CF-Connecting-IP`; CrabIndex доверяет ему, потому что соединение приходит с loopback.

## Что делает CrabIndex с заголовками прокси

- `X-Forwarded-For` (последнее значение), `X-Forwarded-Proto`, `CF-Connecting-IP`, `X-Real-IP` учитываются, **только** если TCP-соединение пришло с loopback. С любого другого адреса они игнорируются - подменить IP клиента нельзя.
- `X-Forwarded-Proto` используется, например, чтобы `/opensearch.xml` отдавал ссылки с `https://`.
- `X-Forwarded-Proto: https` от прокси на том же хосте включает флаг `Secure` у cookie [админ-панели](../admin.md).
- Наличие любого proxy-заголовка при подключении с локального адреса означает «запрос через прокси» → для административных путей нужен `devkey`.

:::warning[Внимание]
Не настраивайте прокси так, чтобы он удалял все forwarding-заголовки: тогда CrabIndex увидит только локальный адрес прокси и пропустит внешние запросы к `/cron/*`, `/dev/*` и `/jsondb*` как локальные.
:::

## 3. Проверьте

```bash
curl -f https://crabindex.example.com/health
curl -f -H "X-Api-Key: ключ-для-клиентов" \
  "https://crabindex.example.com/api/v2.0/indexers/all/results?query=test"
curl -f -H "X-Dev-Key: ключ-для-администрирования" \
  "https://crabindex.example.com/cron/maintenance/Status"
```

Без `devkey` административный запрос через прокси вернёт `401` (или `403`, если `devkey` вообще не задан).

## Админ-панель за прокси

Через прокси [админ-панель](../admin.md) работает без дополнительной настройки, если прокси передаёт путь целиком, со строкой запроса и cookie. Приведённые выше конфигурации Nginx, Caddy и cloudflared так и делают. Адрес входа - `https://crabindex.example.com/admin?ТОКЕН` (или ваш `admin.path`).

- Публикуйте панель только по HTTPS: токен в адресе и `devkey` при входе иначе передаются открытым текстом.
- Держите прокси на том же хосте, что и CrabIndex (соединение с loopback). Заголовкам прокси с другого адреса (другая машина, контейнер в bridge-сети) сервер не доверяет. Тогда cookie панели не получат флаг `Secure`, а лимит попыток входа станет общим для всех посетителей: сервер увидит у всех один IP прокси.
- Адрес с токеном попадает в журнал доступа прокси. Ограничьте доступ к журналам или исключите из него путь панели.
- Можно дополнительно закрыть путь панели на прокси (например, разрешить его только из VPN или по IP).

Пример для Nginx: не писать в журнал запросы к панели и пускать к ней только из своей сети.

```nginx
location /my-panel {
    access_log off;
    allow 203.0.113.0/24;
    deny all;
    proxy_pass http://127.0.0.1:9117;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

## Cron на том же хосте

Cron обращается напрямую к `http://127.0.0.1:9117`, минуя прокси, - поэтому ему `devkey` не нужен. Не направляйте задания cron через публичный адрес.

## Интерфейс на отдельном хостинге

Сайт и админ-панель обращаются к API того же адреса, с которого загружены, и раздаются самим сервером CrabIndex. Отдельно от сервера их не размещают: чтобы открыть интерфейс по своему домену, поставьте перед сервером обратный прокси (или Cloudflare Tunnel), как описано выше.
