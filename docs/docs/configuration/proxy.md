# Прокси

CrabIndex может направлять запросы к трекерам через HTTP- и SOCKS5-прокси. Есть два механизма:

| Блок | Когда применяется |
| --- | --- |
| `globalproxy` | Список правил: прокси выбирается по совпадению URL с регулярным выражением. Действует для **любых** исходящих запросов, независимо от `useproxy` |
| `proxy` | Общий пул для трекеров с `useproxy: true`, если ни одно правило `globalproxy` не подошло |

Порядок выбора для каждого запроса:

1. правила `globalproxy` проверяются по порядку, применяется **первое** совпавшее (с непустым `list`);
2. иначе, если у трекера `useproxy: true` и `proxy.list` не пуст, берётся случайный адрес из пула;
3. иначе запрос идёт напрямую.

## Формат адресов

| Запись | Тип |
| --- | --- |
| `ip:port`, `http://ip:port` | HTTP-прокси (CONNECT) |
| `socks5://ip:port` | SOCKS5; DNS-имена разрешаются на стороне прокси (нужно для `.onion`) |
| `socks5h://ip:port`, `socks://ip:port` | То же, что `socks5://` |

:::note[Примечание]
Адрес без схемы считается HTTP-прокси. Для Tor и любого SOCKS5 обязательно указывайте `socks5://`.
:::

## globalproxy

```yaml
globalproxy:
  - pattern: "\\.onion"
    useAuth: false
    list:
      - socks5://127.0.0.1:9050
```

| Параметр | Описание |
| --- | --- |
| `pattern` | Регулярное выражение, проверяется по полному URL без учёта регистра |
| `list` | Адреса прокси. Все адреса правила перемешиваются и пробуются по очереди, пока один не ответит |
| `useAuth` | Передавать `username` / `password` |
| `username`, `password` | Учётные данные прокси |

Так настроен шаблон `Data/example.yaml`: все запросы к `.onion` идут через локальный Tor.

## proxy

```yaml
proxy:
  useAuth: true
  username: "proxyuser"
  password: "proxypass"
  list:
    - 10.0.0.1:8080
    - socks5://10.0.0.2:1080
```

Для каждого запроса трекера с `useproxy: true` выбирается случайный адрес из `list`.

```yaml
NNMClub:
  useproxy: true
```

:::note[Примечание]
Ключи `proxy.pattern` и `BypassOnLocal` есть в схеме конфигурации, но на выбор прокси не влияют: для отбора по URL используйте `globalproxy`.
:::

## Примеры

### Tor для onion-зеркал

```yaml
globalproxy:
  - pattern: "\\.onion"
    list:
      - socks5://127.0.0.1:9050

Rutor:
  alias: http://rutorxxxxxxxxxxxx.onion   # запросы на onion, URL в базе - на host
```

### Отдельный прокси для одного домена

```yaml
globalproxy:
  - pattern: "kinozal\\.guru"
    useAuth: true
    username: "proxyuser"
    password: "proxypass"
    list:
      - http://10.0.0.1:8080
```

### Весь исходящий трафик через прокси

```yaml
globalproxy:
  - pattern: ".*"
    list:
      - socks5://10.0.0.2:1080
```

Учтите, что такое правило затронет и запросы к Alloha и `syncapi`.

## Что не проходит через эти прокси

- **FlareSolverr** использует свой прокси - переменную окружения `PROXY_URL` его контейнера.
- **cffetch** использует `cffetch.proxy` (или `CFFETCH_PROXY` контейнера).

См. [FlareSolverr и cffetch](flaresolverr.md).

:::tip[Совет]
В Docker Compose, если Tor запущен отдельным контейнером в той же сети, укажите `socks5://tor:9050` вместо `socks5://127.0.0.1:9050`.
:::
