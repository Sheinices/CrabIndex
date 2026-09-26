// Base dictionary (Russian). This is the source of truth for translation keys: every key the
// admin panel uses must exist here. Other languages may omit keys and fall back to these.
//
// Placeholders use `{name}` syntax and are filled by t(key, { name: ... }).
export default {
  // App shell / navigation
  app_subtitle: 'Админ-панель',
  app_title_suffix: 'CrabIndex - админ-панель',
  nav_sections: 'Разделы',
  nav_overview: 'Обзор',
  nav_trackers: 'Трекеры',
  nav_jobs: 'Задачи',
  nav_flaresolverr: 'FlareSolverr',
  nav_settings: 'Настройки',
  nav_waf: 'WAF',
  nav_maintenance: 'Обслуживание',
  nav_logs: 'Логи',
  nav_update: 'Обновление',
  update_available: 'Доступна новая версия',

  // Sidebar / header controls
  open_site: 'Открыть сайт',
  theme_light: 'Светлая тема',
  theme_dark: 'Тёмная тема',
  toggle_theme: 'Сменить тему',
  logout: 'Выйти',
  menu: 'Меню',
  close_menu: 'Закрыть меню',
  skip_to_content: 'К содержимому',
  language: 'Язык',

  // Gate (auth / connection states)
  loading: 'Загрузка…',
  server_unreachable: 'Не удалось связаться с сервером',
  retry: 'Повторить',

  // Common
  refresh: 'Обновить',
  enable: 'Включить',
  more: 'Подробнее',
  docs: 'Документация',

  // Overview page
  ov_updates_every_10s: 'Обновляется каждые 10 секунд',
  ov_metrics: 'Показатели',
  ov_version: 'Версия',
  ov_uptime: 'Аптайм',
  ov_listening: 'Слушает {addr}',
  ov_torrents: 'Раздачи',
  ov_torrents_hint: '{keys} ключей · fast {fast}',
  ov_db_update: 'Обновление БД',
  ov_background_jobs: 'Фоновые задачи',
  ov_all_jobs: 'Все задачи',
  ov_quick_links: 'Быстрые ссылки',

  // Background jobs list (shared with Jobs page)
  jobs_empty: 'Нет активных задач',
  jobs_empty_hint:
    'Это нормально. Здесь видны только долгие задачи, которые идут прямо сейчас: полный обход трекера (ParseAll), составление карт задач (UpdateTasksParse), догрузка старых раздач. Они запускаются по расписанию, в основном ночью и утром по UTC, поэтому после установки или перезапуска данные появляются не сразу. Обычный парсинг каждые 15 минут занимает секунды и сюда не попадает.',

  // WAF summary card
  waf_stats_unavailable: 'Статистика недоступна: {msg}',
  waf_disabled: 'WAF выключен',
  waf_requests_60m: 'Запросов за 60 мин',
  waf_blocked: 'Заблокировано',

  // Login page
  login_subtitle: 'Вход в админ-панель',
  login_devkey: 'Dev-ключ',
  login_show_key: 'Показать ключ',
  login_hide_key: 'Скрыть ключ',
  login_submit: 'Войти',
  login_hint_prefix: 'Ключ - значение',
  login_hint_from: 'из',
  login_hint_suffix:
    'При первом запуске сервер генерирует его и печатает в лог; напомнить адрес и ключ можно командой',
  login_err_invalid: 'Неверный ключ',
  login_err_ratelimit: 'Слишком много попыток, подождите несколько минут',
  login_err_unreachable: 'Сервер недоступен',
  login_err_generic: 'Не удалось войти',

  // Sync card
  sync_title: 'Синхронизация',
  sync_status: 'Статус',
  sync_on: 'включена',
  sync_off: 'выключена',
  sync_source: 'Источник',
  sync_last: 'Последняя',
  sync_fill: 'Наполнение',
  sync_trackers_enabled: '{enabled} / {total} включены',
  sync_config: 'Конфиг',
}
