// English translation. Keys missing here fall back to the Russian base (./ru.js).
export default {
  // App shell / navigation
  app_subtitle: 'Admin panel',
  app_title_suffix: 'CrabIndex - admin panel',
  nav_sections: 'Sections',
  nav_overview: 'Overview',
  nav_trackers: 'Trackers',
  nav_jobs: 'Jobs',
  nav_flaresolverr: 'FlareSolverr',
  nav_settings: 'Settings',
  nav_waf: 'WAF',
  nav_maintenance: 'Maintenance',
  nav_logs: 'Logs',
  nav_update: 'Update',
  update_available: 'A new version is available',

  // Sidebar / header controls
  open_site: 'Open site',
  theme_light: 'Light theme',
  theme_dark: 'Dark theme',
  toggle_theme: 'Toggle theme',
  logout: 'Log out',
  menu: 'Menu',
  close_menu: 'Close menu',
  skip_to_content: 'Skip to content',
  language: 'Language',

  // Gate (auth / connection states)
  loading: 'Loading…',
  server_unreachable: 'Could not reach the server',
  retry: 'Retry',

  // Common
  refresh: 'Refresh',
  enable: 'Enable',
  more: 'Details',
  docs: 'Documentation',

  // Overview page
  ov_updates_every_10s: 'Refreshes every 10 seconds',
  ov_metrics: 'Metrics',
  ov_version: 'Version',
  ov_uptime: 'Uptime',
  ov_listening: 'Listening on {addr}',
  ov_torrents: 'Torrents',
  ov_torrents_hint: '{keys} keys · fast {fast}',
  ov_db_update: 'DB update',
  ov_background_jobs: 'Background jobs',
  ov_all_jobs: 'All jobs',
  ov_quick_links: 'Quick links',

  // Background jobs list (shared with Jobs page)
  jobs_empty: 'No active jobs',
  jobs_empty_hint:
    'This is normal. Only long-running jobs that are in progress right now show here: a full tracker crawl (ParseAll), task-map building (UpdateTasksParse), and backfill of old releases. They run on a schedule, mostly at night and in the morning UTC, so data does not appear immediately after install or restart. The regular parse every 15 minutes takes seconds and is not listed here.',

  // WAF summary card
  waf_stats_unavailable: 'Statistics unavailable: {msg}',
  waf_disabled: 'WAF is off',
  waf_requests_60m: 'Requests in 60 min',
  waf_blocked: 'Blocked',

  // Login page
  login_subtitle: 'Admin panel sign-in',
  login_devkey: 'Dev key',
  login_show_key: 'Show key',
  login_hide_key: 'Hide key',
  login_submit: 'Sign in',
  login_hint_prefix: 'The key is the value of',
  login_hint_from: 'in',
  login_hint_suffix:
    'On first start the server generates it and prints it to the log; you can recall the address and key with the command',
  login_err_invalid: 'Invalid key',
  login_err_ratelimit: 'Too many attempts, wait a few minutes',
  login_err_unreachable: 'Server unavailable',
  login_err_generic: 'Could not sign in',

  // Sync card
  sync_title: 'Synchronization',
  sync_status: 'Status',
  sync_on: 'on',
  sync_off: 'off',
  sync_source: 'Source',
  sync_last: 'Last',
  sync_fill: 'Fill',
  sync_trackers_enabled: '{enabled} / {total} enabled',
  sync_config: 'Config',
}
