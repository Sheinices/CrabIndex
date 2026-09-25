/** `/dev/*` diagnostics and migrations (crates/crab-ops/src/dev). Paths are relative to `{base}/api/`. */

export const DIAGNOSTICS = [
  {
    path: 'dev/findcorrupt',
    label: 'Повреждённые записи',
    description: 'Ищет битые бакеты FileDB (только чтение).',
    params: [{ name: 'samplesize', label: 'Размер выборки', type: 'number', placeholder: '20' }],
  },
  {
    path: 'dev/findduplicatekeys',
    label: 'Дубликаты ключей',
    description: 'Раздачи с одинаковым ключом в разных бакетах (только чтение).',
    params: [
      { name: 'tracker', label: 'Трекер', type: 'text', placeholder: 'все' },
      { name: 'excludenumeric', label: 'Исключить числовые (true/false)', type: 'text', placeholder: 'true' },
    ],
  },
  {
    path: 'dev/findemptysearchfields',
    label: 'Пустые поля поиска',
    description: 'Записи с пустыми _sn/_so (только чтение).',
    params: [{ name: 'samplesize', label: 'Размер выборки', type: 'number', placeholder: '20' }],
  },
]

export const MIGRATIONS = [
  { path: 'dev/updatesize', label: 'Пересчитать размеры', description: 'Пересчитывает size из sizeName для всех раздач.' },
  { path: 'dev/resetchecktime', label: 'Сбросить checkTime', description: 'Ставит checkTime = вчера - все раздачи будут перепроверены.' },
  { path: 'dev/updatedetails', label: 'Обновить детали', description: 'Пересчитывает качество/озвучки/тип для всех раздач.' },
  { path: 'dev/updatesearchname', label: 'Обновить поисковые имена', description: 'Пересобирает _sn/_so и переносит записи при смене ключа.' },
  { path: 'dev/fixemptysearchfields', label: 'Заполнить пустые _sn/_so', description: 'Заполняет пустые поисковые поля и переносит записи.' },
  { path: 'dev/removenullvalues', label: 'Удалить null-записи', description: 'Удаляет строки, сохранённые как null.' },
  { path: 'dev/fixknabennames', label: 'knaben: имена', description: 'Имя/год/название из сохранённого заголовка.' },
  { path: 'dev/fixbitrunames', label: 'bitru: имена', description: 'Убирает сезон/качество из name/originalname.' },
  { path: 'dev/fixrudubrelased', label: 'rudub: год выпуска', description: 'Заполняет relased и обрезанные имена из заголовков.' },
  { path: 'dev/migrateanilibertyurls', label: 'aniliberty: URL', description: 'Добавляет hash=<btih> к URL без него.' },
  { path: 'dev/removeduplicateaniliberty', label: 'aniliberty: дубликаты', description: 'Оставляет самую новую запись на infohash.' },
  { path: 'dev/fixanimelayerduplicates', label: 'animelayer: дубликаты', description: 'Переносит http:// записи на https://, удаляя дубли.' },
  { path: 'dev/fixkinozaldomainduplicates', label: 'kinozal: дубли доменов', description: 'Сводит записи со старых доменов к текущему host.' },
  { path: 'dev/fixultradoxdomainduplicates', label: 'ultradox: дубли доменов', description: 'Сводит записи со старых доменов к текущему host.' },
  {
    path: 'dev/removebucket',
    label: 'Удалить/перенести бакет',
    description: 'Удаляет бакет или переносит его записи под новый ключ name:originalname.',
    params: [
      { name: 'key', label: 'Ключ бакета', type: 'text', placeholder: 'name:originalname', required: true },
      { name: 'migratename', label: 'Перенести в name', type: 'text', placeholder: 'необязательно' },
      { name: 'migrateoriginalname', label: 'Перенести в originalname', type: 'text', placeholder: 'необязательно' },
    ],
  },
]

export const CHECK_MODES = [
  { id: 'report', label: 'Отчёт', description: 'Только проверка, без изменений.', destructive: false },
  { id: 'safe', label: 'Безопасное исправление', description: 'Удаляет null-строки, заполняет пустые поля поиска, переносит записи; сохраняет БД и перестраивает индекс.', destructive: true },
  { id: 'full', label: 'Полное исправление', description: 'Всё из безопасного режима плюс удаление ключей без файлов, перенос и удаление некорректных записей.', destructive: true },
]
