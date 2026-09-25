# Документация

Эта документация - сайт на [Docusaurus](https://docusaurus.io/) (React) в каталоге `docs/` репозитория. Конфиги и компоненты написаны на JavaScript. Сайт собирается в `wwwroot/docs/`, и сервер CrabIndex отдаёт его по адресу `http://<host>:9117/docs/`.

## Структура

```text
docs/
├── docusaurus.config.js   # настройки сайта: baseUrl /docs/, навбар, футер, поиск, Mermaid
├── sidebars.js            # три боковых меню: Руководство, Трекеры, API
├── docs/                  # страницы (.md и .mdx)
│   ├── index.mdx          # главная: шапка, сетка трекеров, карточки
│   ├── trackers/          # по странице на трекер + overview.mdx
│   └── api-reference/
├── src/
│   ├── components/        # React-компоненты: Hero, Cards, TrackerGrid, Endpoint
│   ├── data/trackers.js   # каталог трекеров для TrackerGrid
│   ├── theme/MDXComponents.js  # компоненты, доступные в .mdx без import
│   └── css/custom.css     # цвета темы
└── static/img/            # логотип, иконки трекеров
```

## Предпросмотр

Нужен Node.js 20+.

```bash
make docs-serve        # или: cd docs && npm ci && npm start
```

Откройте `http://localhost:3001/docs/`. Страницы обновляются при сохранении, сервер CrabIndex для предпросмотра не нужен.

## Сборка

```bash
make docs              # или: cd docs && npm run build  → wwwroot/docs/
```

`make web` пересоздаёт весь `wwwroot/` и в конце сам собирает документацию, поэтому после него отдельный `make docs` не нужен. Если собираете документацию отдельно, запускайте её после веб-интерфейса.

Сборка падает на битых ссылках между страницами (`onBrokenLinks: 'throw'`), так что ошибку видно сразу.

## Как писать страницы

- **`.md` - обычный Markdown** (CommonMark). Фигурные скобки и `<host>` в тексте безопасны.
- **`.mdx` - Markdown с React-компонентами.** Используйте для страниц, где нужны вкладки, карточки или интерактив.
- **Плашки:**

  ```md
  :::note
  Текст примечания.
  :::
  ```

  Варианты: `note`, `tip`, `info`, `warning`, `danger`.
- **Диаграммы** - блок кода с языком `mermaid`.
- **Ссылки между страницами** - относительные пути к файлам: `../configuration/overview.md`, `cron.md#установка`.
- **Изображения** - из `docs/static/img/`, путь от корня сайта: `![CrabIndex](/img/logo.png)`.
- **Новая страница** - создайте файл и добавьте его id (путь без расширения) в `sidebars.js`.

### Компоненты в `.mdx`

Доступны без `import`:

```mdx
<Tabs>
  <TabItem value="docker" label="Docker">…</TabItem>
  <TabItem value="linux" label="Linux">…</TabItem>
</Tabs>

<Endpoint method="GET" path="/api/v1.0/torrents">Поиск по FileDB</Endpoint>

<Cards items={[{ icon: '🚀', title: 'Быстрый старт', text: '…', to: '/quickstart/' }]} />

<TrackerGrid />
```

## Перед коммитом

1. `make docs` проходит без ошибок.
2. Маршруты, параметры и ключи конфига проверены по коду.
3. Новые страницы добавлены в `sidebars.js`.
