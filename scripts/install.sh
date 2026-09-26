#!/usr/bin/env bash
# CrabIndex installer for Linux + systemd.
#
# Checks and installs the system packages CrabIndex needs (curl, rsync, cron, flock, tar …),
# installs a release bundle (`make dist` output directory, or a .tar.gz/.tar.xz path/URL) or
# builds one from the source tree (Rust via rustup, Node.js for the web UI), into
# /opt/crabindex, creates the `crabindex` system user, a systemd unit and the user's crontab
# (Data/crontab), and prepares the admin panel: URL path, entry token and devkey (the admin
# password) are written into /opt/crabindex/init.yaml.
#
# Usage:
#   sudo scripts/install.sh [--bundle DIR|FILE|URL | --from-source [DIR]] [--admin-path /x]
#                           [--port N] [--db sync|parse] [--flaresolverr | --no-flaresolverr]
#                           [--lang ru|en] [--yes]
#   sudo scripts/install.sh --update [--bundle ... | --from-source] [--db sync|parse]
#   sudo scripts/install.sh --uninstall [--purge] [--yes]
#   sudo scripts/install.sh --check
#   curl -fsSL https://raw.githubusercontent.com/sheinices/crabindex/main/scripts/install.sh | sudo bash
#     (no bundle and no sources next to the script → downloads the latest GitHub release)
#
# Environment: INSTALL_DIR (default /opt/crabindex), SERVICE_USER (default crabindex),
# CRABINDEX_LANG (ru|en, interface language when --lang is not given),
# CRABINDEX_DB (sync|parse, database source when --db is not given).
set -euo pipefail

INSTALL_DIR="${INSTALL_DIR:-/opt/crabindex}"
SERVICE_USER="${SERVICE_USER:-crabindex}"
SERVICE_NAME="crabindex"
UNIT_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

RESERVED_PATHS=(/ /api /cron /dev /docs /swagger /sync /stats /torznab /health /version
  /lastupdatedb /jsondb /img /assets /openapi.yaml /opensearch.xml /sw.js
  /manifest.webmanifest /search)

BUNDLE=""
BUNDLE_ROOT=""
CONFIG_IS_JSON=0
ADMIN_PATH_ARG=""
ASSUME_YES=0
MODE="install"
PURGE=0
WORK_DIR=""
FROM_SOURCE=0
CARGO_BIN=""
SOURCE_DIR=""
SKIP_DEPS=0
RELEASE_TAG=""
RELEASE_REPO="${CRABINDEX_REPO:-sheinices/crabindex}"
PKG_MGR=""
LISTEN_PORT=9117
# --port value ("" = default 9117, or a free one on a fresh install when 9117 is busy).
PORT_ARG=""
# 1 = write LISTEN_PORT into init.yaml (fresh install with a busy default port, or --port).
PORT_WRITE=0
# Cloudflare bypass (Docker): "" = ask on a fresh install, 1 = install/recreate, 0 = skip.
WITH_FLARESOLVERR=""
# Database source: "" = ask on a fresh install / keep on update, "sync" or "parse".
DB_MODE=""
DB_GIVEN=0
# 1 = apply DB_MODE (syncapi, crontab, FlareSolverr): fresh config or explicit --db / CRABINDEX_DB.
DB_APPLY=0
# Crontab flavour to install: "sync" = reduced (no tracker jobs), anything else = full.
CRONTAB_DB_MODE=""
DB_SYNC_URL="https://sync.crab.rip"
DB_SYNC_MARKER="# crabindex-db: sync"
# 1 = init.yaml was created from Data/example.yaml during this run.
CONFIG_CREATED=0
SYS_MEM_GB=""
SYS_CORES=""
FLARESOLVERR_IMAGE="${FLARESOLVERR_IMAGE:-ghcr.io/flaresolverr/flaresolverr:latest}"
# Empty = picked from the server's cores and memory (see set_flaresolverr_limits).
FLARESOLVERR_CPUS="${FLARESOLVERR_CPUS:-}"
FLARESOLVERR_MEMORY="${FLARESOLVERR_MEMORY:-}"
CFFETCH_IMAGE="${CFFETCH_IMAGE:-ghcr.io/jacred-fdb/cffetch:latest}"
CFFETCH_CPUS="${CFFETCH_CPUS:-0.5}"
CFFETCH_MEMORY="${CFFETCH_MEMORY:-256m}"
MANAGED_LABEL="crabindex.managed=1"

# Interface language: "ru" or "en" (see the language selection below).
UI_LANG="en"
# 1 = no --lang / CRABINDEX_LANG: ask interactively once arguments are parsed.
UI_LANG_ASK=0

# t <russian> <english>: the text in the interface language.
t() {
  if [[ "$UI_LANG" == en ]]; then printf '%s' "$2"; else printf '%s' "$1"; fi
}

info() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m%s\033[0m %s\n' "$(t 'внимание:' 'warning:')" "$*" >&2; }
die() {
  printf '\033[1;31m%s\033[0m %s\n' "$(t 'ошибка:' 'error:')" "$*" >&2
  exit 1
}

# Default language from the locale: ru* -> ru, anything else -> en.
locale_lang() {
  case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
    ru*) printf 'ru' ;;
    *) printf 'en' ;;
  esac
}

# Language selection, step 1 (before parsing, so errors and --help use it):
# --lang ru|en (or --lang=ru), else CRABINDEX_LANG, else the locale (asked later if interactive).
lang_arg=""
lang_given=0
lang_next=0
for a in "$@"; do
  if [[ "$lang_next" -eq 1 ]]; then
    lang_arg="$a"
    lang_next=0
    continue
  fi
  case "$a" in
    --lang)
      lang_given=1
      lang_next=1
      ;;
    --lang=*)
      lang_given=1
      lang_arg="${a#*=}"
      ;;
  esac
done
if [[ "$lang_given" -eq 1 ]]; then
  case "$lang_arg" in
    ru | en) UI_LANG="$lang_arg" ;;
    *)
      printf '\033[1;31m%s\033[0m %s\n' "ошибка / error:" "--lang: ru или en / ru or en" >&2
      exit 1
      ;;
  esac
elif [[ "${CRABINDEX_LANG:-}" == ru || "${CRABINDEX_LANG:-}" == en ]]; then
  UI_LANG="$CRABINDEX_LANG"
else
  UI_LANG="$(locale_lang)"
  UI_LANG_ASK=1
fi
unset lang_arg lang_given lang_next a

cleanup() {
  if [[ -n "$WORK_DIR" && -d "$WORK_DIR" ]]; then
    rm -rf "$WORK_DIR"
  fi
}
trap cleanup EXIT

usage() {
  if [[ "$UI_LANG" == en ]]; then
    cat <<'EOF'
CrabIndex installer (Linux + systemd).

  sudo install.sh [options]

Options:
  --bundle PATH|URL   `make dist` directory or a .tar.gz/.tar.xz archive (path or URL)
                      default: the script's directory (if it contains crabindex) or ../dist
  --from-source [DIR] build from source (default: the repository the script is in);
                      missing gcc, pkg-config, git, Rust (rustup) and Node.js are installed
  --version vX.Y.Z    download this release from GitHub (default: the latest one, if there
                      is neither a bundle nor sources next to the script)
  --admin-path /x     admin panel path without asking (one segment [a-z0-9_-], 2-32 chars)
  --port N            server port (listenport, 1024-65535, default 9117); if the port is
                      used by another program, the installation stops. Without --port, on a
                      fresh install with 9117 busy, the installer offers the nearest free port
                      (9118-9199), with --yes it picks it itself. --update keeps the port from init.yaml
  --lang ru|en        interface language (default: CRABINDEX_LANG, otherwise asked at start;
                      without a terminal or with --yes - from the system locale)
  --db sync|parse     database source on a fresh install: sync - a ready database is
                      downloaded from sync.crab.rip and kept up to date (default), parse -
                      own tracker parsing (full crontab, FlareSolverr is offered). Also
                      CRABINDEX_DB. With --update it switches an existing installation
  --yes, -y           ask no questions (path /admin unless --admin-path is given)
  --update            update the installed version (config and data are kept)
  --uninstall         remove the service, crontab, binary and web UI (data is kept)
  --purge             with --uninstall: also remove data, config and the user
  --check             only check the system and show what is missing
  --no-deps           do not install system packages (check only)
  --flaresolverr      install FlareSolverr and cffetch in Docker to bypass Cloudflare
                      (rutracker, kinozal and others) with CPU and memory limits
  --no-flaresolverr   do not install and do not ask
  -h, --help          show this help

Missing packages (curl, ca-certificates, tar, xz, gzip, rsync, cron, util-linux/flock,
iproute2) are installed automatically via apt, dnf, yum, zypper, pacman or apk.

FlareSolverr runs a real Chrome browser and needs a lot of resources. Container limits
are picked from the server: FLARESOLVERR_MEMORY - 1536m (up to 6 GB of memory), 3g (up to 12 GB),
4g (more); FLARESOLVERR_CPUS - 1 (up to 2 cores), 1.5 (3-5 cores), 2 (6+ cores). The environment
variables FLARESOLVERR_CPUS, FLARESOLVERR_MEMORY, CFFETCH_CPUS (0.5), CFFETCH_MEMORY (256m)
set the limits explicitly. At least 2 cores and 4 GB of memory are recommended.
EOF
    return
  fi
  cat <<'EOF'
Установка CrabIndex (Linux + systemd).

  sudo install.sh [параметры]

Параметры:
  --bundle PATH|URL   каталог `make dist` или архив .tar.gz/.tar.xz (путь или URL)
                      по умолчанию: каталог скрипта (если в нём есть crabindex) или ../dist
  --from-source [DIR] собрать из исходников (по умолчанию - репозиторий, где лежит скрипт);
                      недостающие gcc, pkg-config, git, Rust (rustup) и Node.js ставятся сами
  --version vX.Y.Z    скачать этот релиз с GitHub (по умолчанию - последний, если нет
                      ни комплекта, ни исходников рядом со скриптом)
  --admin-path /x     путь админ-панели без вопросов (один сегмент [a-z0-9_-], 2-32 символа)
  --port N            порт сервера (listenport, 1024-65535, по умолчанию 9117); если порт
                      занят другой программой, установка прерывается. Без --port при новой
                      установке и занятом 9117 установщик предложит ближайший свободный
                      (9118-9199), с --yes выберет его сам. --update сохраняет порт из init.yaml
  --lang ru|en        язык интерфейса (по умолчанию CRABINDEX_LANG, иначе вопрос при запуске;
                      без терминала или с --yes - по локали системы)
  --db sync|parse     источник базы при новой установке: sync - готовая база скачивается
                      с sync.crab.rip и обновляется сама (по умолчанию), parse - собственный
                      парсинг трекеров (полный crontab, предлагается FlareSolverr). Также
                      CRABINDEX_DB. С --update переключает существующую установку
  --yes, -y           не задавать вопросов (путь /admin, если не указан --admin-path)
  --update            обновить установленную версию (конфиг и данные сохраняются)
  --uninstall         удалить службу, crontab, бинарник и веб-интерфейс (данные остаются)
  --purge             вместе с --uninstall: удалить также данные, конфиг и пользователя
  --check             только проверить систему и показать, чего не хватает
  --no-deps           не устанавливать системные пакеты (только проверить)
  --flaresolverr      поставить FlareSolverr и cffetch в Docker для обхода Cloudflare
                      (rutracker, kinozal и др.) с ограничением CPU и памяти
  --no-flaresolverr   не ставить и не спрашивать
  -h, --help          эта справка

Недостающие пакеты (curl, ca-certificates, tar, xz, gzip, rsync, cron, util-linux/flock,
iproute2) ставятся автоматически через apt, dnf, yum, zypper, pacman или apk.

FlareSolverr запускает настоящий браузер Chrome и требователен к ресурсам. Лимиты
контейнеров подбираются по серверу: FLARESOLVERR_MEMORY - 1536m (память до 6 ГБ), 3g (до 12 ГБ),
4g (больше); FLARESOLVERR_CPUS - 1 (до 2 ядер), 1.5 (3-5 ядер), 2 (6+ ядер). Переменные
окружения FLARESOLVERR_CPUS, FLARESOLVERR_MEMORY, CFFETCH_CPUS (0.5), CFFETCH_MEMORY (256m)
задают лимиты явно. Рекомендуется от 2 ядер и 4 ГБ памяти.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundle)
      [[ $# -ge 2 ]] || die "$(t '--bundle: укажите путь или URL' '--bundle: specify a path or URL')"
      BUNDLE="$2"
      shift 2
      ;;
    --bundle=*)
      BUNDLE="${1#*=}"
      shift
      ;;
    --admin-path)
      [[ $# -ge 2 ]] || die "$(t '--admin-path: укажите путь' '--admin-path: specify a path')"
      ADMIN_PATH_ARG="$2"
      shift 2
      ;;
    --admin-path=*)
      ADMIN_PATH_ARG="${1#*=}"
      shift
      ;;
    --port)
      [[ $# -ge 2 ]] || die "$(t '--port: укажите номер порта' '--port: specify a port number')"
      PORT_ARG="$2"
      shift 2
      ;;
    --port=*)
      PORT_ARG="${1#*=}"
      shift
      ;;
    -y | --yes)
      ASSUME_YES=1
      shift
      ;;
    --update)
      MODE="update"
      shift
      ;;
    --uninstall)
      MODE="uninstall"
      shift
      ;;
    --purge)
      PURGE=1
      shift
      ;;
    --from-source)
      FROM_SOURCE=1
      shift
      if [[ $# -gt 0 && "$1" != -* ]]; then
        SOURCE_DIR="$1"
        shift
      fi
      ;;
    --from-source=*)
      FROM_SOURCE=1
      SOURCE_DIR="${1#*=}"
      shift
      ;;
    --check)
      MODE="check"
      shift
      ;;
    --version)
      [[ $# -ge 2 ]] || die "$(t '--version: укажите тег, например v1.0.0' '--version: specify a tag, e.g. v1.0.0')"
      RELEASE_TAG="$2"
      shift 2
      ;;
    --version=*)
      RELEASE_TAG="${1#*=}"
      shift
      ;;
    --db)
      [[ $# -ge 2 ]] || die "$(t '--db: укажите sync или parse' '--db: specify sync or parse')"
      DB_MODE="$2"
      DB_GIVEN=1
      shift 2
      ;;
    --db=*)
      DB_MODE="${1#*=}"
      DB_GIVEN=1
      shift
      ;;
    --lang)
      # validated before parsing
      shift 2
      ;;
    --lang=*)
      shift
      ;;
    --no-deps)
      SKIP_DEPS=1
      shift
      ;;
    --flaresolverr)
      WITH_FLARESOLVERR=1
      shift
      ;;
    --no-flaresolverr)
      WITH_FLARESOLVERR=0
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      die "$(t "неизвестный параметр: $1" "unknown option: $1")"
      ;;
  esac
done

# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------

valid_port() { # valid_port <port>: a number 1024-65535 without leading zeros
  [[ "$1" =~ ^[1-9][0-9]{3,4}$ ]] && (($1 >= 1024 && $1 <= 65535))
}

can_prompt() {
  [[ "$ASSUME_YES" -eq 0 ]] && { : </dev/tty; } 2>/dev/null
}

ask() { # ask <prompt> -> reply on stdout (from the terminal even under `curl | bash`)
  local reply=""
  read -r -p "$1" reply </dev/tty || true
  printf '%s' "$reply"
}

# Language selection, step 2: no --lang / CRABINDEX_LANG and a terminal -> ask once,
# defaulting to the locale.
choose_language() {
  local def=2 choice
  [[ "$UI_LANG" == ru ]] && def=1
  {
    echo "Язык / Language:"
    echo "  1) Русский"
    echo "  2) English"
  } >/dev/tty
  while true; do
    choice="$(ask "Выберите / Choose [$def]: ")"
    choice="${choice//[[:space:]]/}"
    case "${choice:-$def}" in
      1)
        UI_LANG="ru"
        break
        ;;
      2)
        UI_LANG="en"
        break
        ;;
      *) echo "Введите 1 или 2 / Enter 1 or 2." >/dev/tty ;;
    esac
  done
  echo >/dev/tty
}

if [[ "$UI_LANG_ASK" -eq 1 ]] && can_prompt; then
  choose_language
fi

if [[ "$DB_GIVEN" -eq 1 ]]; then
  case "$DB_MODE" in
    sync | parse) ;;
    *) die "$(t "--db: недопустимое значение «${DB_MODE}» (sync или parse)" "--db: invalid value \"${DB_MODE}\" (sync or parse)")" ;;
  esac
elif [[ -n "${CRABINDEX_DB:-}" ]]; then
  case "$CRABINDEX_DB" in
    sync | parse) DB_MODE="$CRABINDEX_DB" ;;
    *) die "$(t "CRABINDEX_DB: недопустимое значение «${CRABINDEX_DB}» (sync или parse)" "CRABINDEX_DB: invalid value \"${CRABINDEX_DB}\" (sync or parse)")" ;;
  esac
fi

if [[ -n "$PORT_ARG" ]] && ! valid_port "$PORT_ARG"; then
  die "$(t "--port: недопустимый порт «${PORT_ARG}» (число от 1024 до 65535)" "--port: invalid port \"${PORT_ARG}\" (a number from 1024 to 65535)")"
fi

confirm() { # confirm <prompt>; --yes answers "yes"
  if [[ "$ASSUME_YES" -eq 1 ]]; then
    return 0
  fi
  can_prompt || return 1
  local r
  r="$(ask "$1 [y/N]: ")"
  [[ "$r" =~ ^([yY]|[дД]|yes|да)$ ]]
}

# Same rules as the server: one segment [a-z0-9_-]{2,32}, not a reserved path.
normalize_admin_path() {
  local p="$1"
  p="${p#"${p%%[![:space:]]*}"}"
  p="${p%"${p##*[![:space:]]}"}"
  p="${p#/}"
  p="${p%/}"
  [[ "$p" =~ ^[a-z0-9_-]{2,32}$ ]] || return 1
  local r
  for r in "${RESERVED_PATHS[@]}"; do
    [[ "/$p" == "$r" ]] && return 1
  done
  printf '/%s' "$p"
}

random_alnum() { # random_alnum <length>, from /dev/urandom
  local len="$1" s=""
  while [[ ${#s} -lt $len ]]; do
    s+="$(head -c 4096 /dev/urandom | LC_ALL=C tr -dc 'A-Za-z0-9')"
  done
  printf '%s' "${s:0:$len}"
}

# Value of a top-level YAML key (quotes and trailing comment removed).
yaml_top_get() {
  local key="$1" file="$2"
  awk -v key="$key" '
    index($0, key ":") == 1 {
      v = substr($0, length(key) + 2)
      sub(/[[:space:]]+#.*$/, "", v)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", v)
      gsub(/^["\047]|["\047]$/, "", v)
      print v
      exit
    }' "$file"
}

# Value of `<section>.<key>` from a block-style top-level section.
yaml_section_get() { # yaml_section_get <section> <key> <file>
  local section="$1" key="$2" file="$3"
  awk -v section="$section" -v key="$key" '
    /^[^[:space:]#][^:]*:/ { in_sec = ($0 ~ ("^" section ":[[:space:]]*(#.*)?$")); next }
    in_sec && $0 ~ ("^[[:space:]]+" key ":") {
      v = $0
      sub("^[[:space:]]+" key ":", "", v)
      sub(/[[:space:]]+#.*$/, "", v)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", v)
      gsub(/^["\047]|["\047]$/, "", v)
      print v
      exit
    }' "$file"
}

yaml_admin_get() { yaml_section_get admin "$1" "$2"; }

# Rewrite a file in place from stdin, keeping owner and mode.
replace_content() {
  local file="$1" tmp
  tmp="$(mktemp)"
  cat >"$tmp"
  cat "$tmp" >"$file"
  rm -f "$tmp"
}

yaml_top_set() { # yaml_top_set <key> <value> <file>
  local key="$1" value="$2" file="$3"
  if grep -q "^${key}:" "$file"; then
    awk -v key="$key" -v val="$value" '
      !done && index($0, key ":") == 1 { print key ": " val; done = 1; next }
      { print }' "$file" | replace_content "$file"
  else
    printf '%s: %s\n' "$key" "$value" >>"$file"
  fi
}

yaml_section_set() { # yaml_section_set <section> <key> <value> <file>
  local section="$1" key="$2" value="$3" file="$4"
  awk -v section="$section" -v key="$key" -v val="$value" '
    function flush() { if (in_sec && !done) { print "  " key ": " val; done = 1 } }
    /^[^[:space:]#][^:]*:/ {
      flush()
      in_sec = ($0 ~ ("^" section ":[[:space:]]*(#.*)?$"))
      if (in_sec) seen = 1
      print
      next
    }
    in_sec && !done && $0 ~ ("^[[:space:]]+" key ":") { print "  " key ": " val; done = 1; next }
    { print }
    END {
      flush()
      if (!seen) { print ""; print section ":"; print "  " key ": " val }
    }' "$file" | replace_content "$file"
}

yaml_admin_set() { yaml_section_set admin "$1" "$2" "$3"; }

server_ip() {
  local ip=""
  if command -v ip >/dev/null 2>&1; then
    ip="$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{for (i = 1; i < NF; i++) if ($i == "src") { print $(i + 1); exit }}')"
  fi
  if [[ -z "$ip" ]] && command -v hostname >/dev/null 2>&1; then
    ip="$(hostname -I 2>/dev/null | awk '{print $1}')"
  fi
  printf '%s' "${ip:-127.0.0.1}"
}

# ---------------------------------------------------------------------------
# listen port
# ---------------------------------------------------------------------------

port_in_use() { # port_in_use <port>: something listens on this TCP port
  local port="$1"
  if command -v ss >/dev/null 2>&1; then
    ss -tlnH 2>/dev/null | awk '{print $4}' | grep -qE "[:.]${port}\$"
  elif command -v netstat >/dev/null 2>&1; then
    netstat -tln 2>/dev/null | awk 'NR > 2 {print $4}' | grep -qE "[:.]${port}\$"
  else
    # No ss/netstat (iproute2 is installed later): try to connect on localhost.
    (exec 3<>"/dev/tcp/127.0.0.1/${port}") 2>/dev/null
  fi
}

config_listen_port() { # listenport from the installed init.yaml (empty if absent or invalid)
  local cfg="$INSTALL_DIR/init.yaml" p=""
  [[ -r "$cfg" ]] && p="$(yaml_top_get listenport "$cfg" 2>/dev/null || true)"
  if valid_port "$p"; then printf '%s' "$p"; fi
}

own_port() { # own_port <port>: the port is held by the running crabindex service itself
  systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null &&
    [[ "$1" == "$(config_listen_port)" || ( "$1" == 9117 && -z "$(config_listen_port)" ) ]]
}

suggest_free_port() { # nearest free port after 9117, empty if 9118-9199 are all busy
  local p
  for ((p = 9118; p <= 9199; p++)); do
    if ! port_in_use "$p"; then
      printf '%s' "$p"
      return 0
    fi
  done
}

ask_listen_port() { # ask_listen_port <busy port> <suggestion or ""> -> chosen free port on stdout
  local busy="$1" suggested="$2" reply
  {
    echo
    t "Порт $busy уже занят другой программой." "Port $busy is already used by another program."
    echo
    if [[ -n "$suggested" ]]; then
      t "  Ближайший свободный - $suggested. Enter - принять, или введите другой порт (1024-65535)." \
        "  The nearest free one is $suggested. Press Enter to accept, or type another port (1024-65535)."
    else
      t "  В диапазоне 9118-9199 свободных портов нет. Введите порт (1024-65535)." \
        "  No free ports in the 9118-9199 range. Enter a port (1024-65535)."
    fi
    echo
  } >/dev/tty
  while true; do
    reply="$(ask "$(t 'Порт' 'Port')${suggested:+ [$suggested]}: ")"
    reply="${reply//[[:space:]]/}"
    reply="${reply:-$suggested}"
    if ! valid_port "$reply"; then
      t "Введите число от 1024 до 65535." "Enter a number from 1024 to 65535." >/dev/tty
      echo >/dev/tty
    elif port_in_use "$reply"; then
      t "Порт $reply тоже занят, выберите другой." "Port $reply is busy too, choose another one." >/dev/tty
      echo >/dev/tty
    else
      printf '%s' "$reply"
      return
    fi
  done
}

# Port for system_check: --port, else listenport from an existing init.yaml, else 9117.
guess_listen_port() {
  local p
  p="$(config_listen_port)"
  LISTEN_PORT="${PORT_ARG:-${p:-9117}}"
}

# Sets LISTEN_PORT (and PORT_WRITE=1 when init.yaml must get it). Runs before the service is
# stopped on update, so a port held by crabindex itself is recognised as its own.
resolve_listen_port() {
  local current
  current="$(config_listen_port)"
  if [[ -n "$PORT_ARG" ]]; then
    if port_in_use "$PORT_ARG" && ! own_port "$PORT_ARG"; then
      die "$(t "порт $PORT_ARG (--port) занят другой программой - освободите его или укажите другой --port" \
        "port $PORT_ARG (--port) is used by another program - free it or pass another --port")"
    fi
    LISTEN_PORT="$PORT_ARG"
    PORT_WRITE=1
    return
  fi
  # update, or a config left from a previous installation: keep its port
  if [[ "$MODE" == "update" || -f "$INSTALL_DIR/init.yaml" || -f "$INSTALL_DIR/init.conf" ]]; then
    LISTEN_PORT="${current:-9117}"
    return
  fi
  LISTEN_PORT=9117
  port_in_use "$LISTEN_PORT" || return 0
  local suggested
  suggested="$(suggest_free_port)"
  if can_prompt; then
    LISTEN_PORT="$(ask_listen_port 9117 "$suggested")"
  else
    [[ -n "$suggested" ]] || die "$(t "порт 9117 занят, а в 9118-9199 нет свободных - укажите порт: --port N" \
      "port 9117 is busy and 9118-9199 has no free ports - specify one: --port N")"
    LISTEN_PORT="$suggested"
    info "$(t "Порт 9117 занят - CrabIndex будет слушать свободный порт $LISTEN_PORT" \
      "Port 9117 is busy - CrabIndex will listen on free port $LISTEN_PORT")"
  fi
  PORT_WRITE=1
}

require_root() {
  [[ "${EUID:-$(id -u)}" -eq 0 ]] || die "$(t "запустите от root (sudo $0 ...)" "run as root (sudo $0 ...)")"
}

require_systemd() {
  command -v systemctl >/dev/null 2>&1 || die "$(t 'нужен systemd (systemctl не найден)' 'systemd is required (systemctl not found)')"
}

# ---------------------------------------------------------------------------
# system packages
# ---------------------------------------------------------------------------

detect_pkg_mgr() {
  local m
  for m in apt-get dnf yum zypper pacman apk; do
    if command -v "$m" >/dev/null 2>&1; then
      PKG_MGR="$m"
      return 0
    fi
  done
  PKG_MGR=""
}

# pkg_name <logical> -> distro package name for $PKG_MGR (empty = not needed / unknown)
pkg_name() {
  local p="$1"
  case "$PKG_MGR:$p" in
    *:curl | *:rsync | *:tar | *:gzip | *:git) echo "$p" ;;
    *:ca-certificates) echo "ca-certificates" ;;
    apt-get:xz) echo "xz-utils" ;;
    *:xz) echo "xz" ;;
    apt-get:cron) echo "cron" ;;
    dnf:cron | yum:cron) echo "cronie" ;;
    zypper:cron) echo "cronie" ;;
    pacman:cron) echo "cronie" ;;
    apk:cron) echo "dcron" ;;
    *:flock) echo "util-linux" ;;
    apt-get:iproute) echo "iproute2" ;;
    pacman:iproute | apk:iproute) echo "iproute2" ;;
    *:iproute) echo "iproute" ;;
    apt-get:build) echo "build-essential pkg-config" ;;
    dnf:build | yum:build) echo "gcc gcc-c++ make pkgconf-pkg-config" ;;
    zypper:build) echo "gcc gcc-c++ make pkg-config" ;;
    pacman:build) echo "base-devel" ;;
    apk:build) echo "build-base pkgconf" ;;
    *) echo "" ;;
  esac
}

pkg_install() { # pkg_install <distro packages...>
  [[ $# -gt 0 ]] || return 0
  [[ -n "$PKG_MGR" ]] || die "$(t "не найден менеджер пакетов - установите вручную: $*" "no package manager found - install manually: $*")"
  info "$(t "Установка пакетов: $*" "Installing packages: $*")"
  case "$PKG_MGR" in
    apt-get)
      DEBIAN_FRONTEND=noninteractive apt-get update -qq >/dev/null
      DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "$@" >/dev/null
      ;;
    dnf | yum) "$PKG_MGR" install -y -q "$@" >/dev/null ;;
    zypper) zypper --non-interactive -q install "$@" >/dev/null ;;
    pacman) pacman -Sy --noconfirm --needed "$@" >/dev/null ;;
    apk) apk add --no-cache -q "$@" >/dev/null ;;
  esac
}

# command → logical package required at runtime
RUNTIME_DEPS=(curl:curl update-ca-certificates:ca-certificates tar:tar xz:xz gzip:gzip rsync:rsync
  crontab:cron flock:flock ip:iproute)

missing_runtime_pkgs() { # prints distro package names for missing runtime commands
  local item cmd logical name
  for item in "${RUNTIME_DEPS[@]}"; do
    cmd="${item%%:*}"
    logical="${item#*:}"
    if [[ "$logical" == "ca-certificates" ]]; then
      [[ -s /etc/ssl/certs/ca-certificates.crt || -s /etc/pki/tls/certs/ca-bundle.crt || -s /etc/ssl/ca-bundle.pem ]] && continue
    elif command -v "$cmd" >/dev/null 2>&1; then
      continue
    fi
    name="$(pkg_name "$logical")"
    [[ -n "$name" ]] && printf '%s\n' "$name"
  done | sort -u
}

enable_cron_service() {
  local svc
  for svc in cron crond cronie dcron; do
    if systemctl list-unit-files "${svc}.service" >/dev/null 2>&1 \
      && systemctl list-unit-files "${svc}.service" | grep -q "^${svc}\.service"; then
      systemctl enable --now "$svc" >/dev/null 2>&1 || true
      return 0
    fi
  done
}

ensure_runtime_deps() {
  detect_pkg_mgr
  local missing=()
  mapfile -t missing < <(missing_runtime_pkgs)
  if [[ ${#missing[@]} -gt 0 ]]; then
    if [[ "$SKIP_DEPS" -eq 1 ]]; then
      warn "$(t "не хватает пакетов (--no-deps): ${missing[*]}" "missing packages (--no-deps): ${missing[*]}")"
    else
      # shellcheck disable=SC2046
      pkg_install $(printf '%s ' "${missing[@]}")
    fi
  fi
  command -v crontab >/dev/null 2>&1 && enable_cron_service
  return 0
}

# ---------------------------------------------------------------------------
# system check
# ---------------------------------------------------------------------------

check_line() { # check_line <ok 0|1> <text> [hint]
  if [[ "$1" -eq 0 ]]; then
    printf '  \033[1;32m✓\033[0m %s\n' "$2"
  else
    printf '  \033[1;31m✗\033[0m %s%s\n' "$2" "${3:+ - $3}"
  fi
}

system_check() { # prints a summary; returns 1 when something required is missing
  detect_pkg_mgr
  local bad=0 item cmd ok
  t "Проверка системы:" "System check:"
  echo
  command -v systemctl >/dev/null 2>&1 && ok=0 || { ok=1; bad=1; }
  check_line "$ok" "systemd" "$(t 'нужен для службы crabindex' 'required for the crabindex service')"
  check_line "$([[ -n "$PKG_MGR" ]] && echo 0 || echo 1)" "$(t 'менеджер пакетов' 'package manager')${PKG_MGR:+: $PKG_MGR}" \
    "$(t 'пакеты придётся ставить вручную' 'packages will have to be installed manually')"
  for item in "${RUNTIME_DEPS[@]}"; do
    cmd="${item%%:*}"
    [[ "$cmd" == "update-ca-certificates" ]] && continue
    command -v "$cmd" >/dev/null 2>&1 && ok=0 || ok=1
    check_line "$ok" "$cmd" "$(t 'будет установлен' 'will be installed') ($(pkg_name "${item#*:}"))"
  done
  local arch
  arch="$(uname -m)"
  case "$arch" in
    x86_64 | aarch64) check_line 0 "$(t 'архитектура' 'architecture') $arch" ;;
    *) check_line 1 "$(t 'архитектура' 'architecture') $arch" "$(t 'поддерживаются x86_64 и aarch64' 'x86_64 and aarch64 are supported')" ;;
  esac
  local free_gb mem_mb
  free_gb="$(df -Pk "$(dirname "$INSTALL_DIR")" 2>/dev/null | awk 'NR==2 {print int($4/1024/1024)}')"
  if [[ -n "$free_gb" ]]; then
    [[ "$free_gb" -ge 10 ]] && ok=0 || ok=1
    check_line "$ok" "$(t "свободно на диске: ${free_gb} ГБ" "free disk space: ${free_gb} GB")" \
      "$(t 'для полной базы нужно 10+ ГБ' 'the full database needs 10+ GB')"
  fi
  mem_mb="$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo 2>/dev/null)"
  if [[ -n "$mem_mb" ]]; then
    [[ "$mem_mb" -ge 1024 ]] && ok=0 || ok=1
    check_line "$ok" "$(t "память: ${mem_mb} МБ" "memory: ${mem_mb} MB")" "$(t 'рекомендуется 1+ ГБ' '1+ GB recommended')"
  fi
  guess_listen_port
  if port_in_use "$LISTEN_PORT"; then
    if own_port "$LISTEN_PORT"; then
      check_line 0 "$(t "порт $LISTEN_PORT занят самим crabindex" "port $LISTEN_PORT is used by crabindex itself")"
    elif [[ -n "$PORT_ARG" ]]; then
      check_line 1 "$(t "порт $LISTEN_PORT занят" "port $LISTEN_PORT is busy")" \
        "$(t 'освободите его или укажите другой --port' 'free it or pass another --port')"
    else
      check_line 1 "$(t "порт $LISTEN_PORT занят" "port $LISTEN_PORT is busy")" \
        "$(t 'при новой установке установщик предложит другой порт (или --port N)' 'on a fresh install the installer will offer another port (or --port N)')"
    fi
  else
    check_line 0 "$(t "порт $LISTEN_PORT свободен" "port $LISTEN_PORT is free")"
  fi
  return "$bad"
}

# ---------------------------------------------------------------------------
# build from source
# ---------------------------------------------------------------------------

source_root_guess() { # repository root next to this script, if it looks like the CrabIndex tree
  local root
  root="$(cd "$SCRIPT_DIR/.." && pwd)"
  [[ -f "$root/Cargo.toml" && -d "$root/crates/crabindex" ]] && printf '%s' "$root"
}

ensure_rust() {
  local cargo="${CARGO_HOME:-$HOME/.cargo}/bin/cargo"
  if command -v cargo >/dev/null 2>&1; then
    CARGO_BIN="$(command -v cargo)"
    return
  fi
  if [[ ! -x "$cargo" ]]; then
    info "$(t 'Установка Rust (rustup, stable)' 'Installing Rust (rustup, stable)')"
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null
  fi
  CARGO_BIN="$cargo"
}

node_major() {
  command -v node >/dev/null 2>&1 || { echo 0; return; }
  node -v 2>/dev/null | sed -E 's/^v([0-9]+).*/\1/'
}

ensure_node() {
  if [[ "$(node_major)" -ge 20 ]] && command -v npm >/dev/null 2>&1; then
    return
  fi
  info "$(t 'Установка Node.js 22 (нужен для сборки веб-интерфейса)' 'Installing Node.js 22 (needed to build the web UI)')"
  case "$PKG_MGR" in
    apt-get)
      curl -fsSL https://deb.nodesource.com/setup_22.x | bash - >/dev/null
      pkg_install nodejs
      ;;
    dnf | yum)
      curl -fsSL https://rpm.nodesource.com/setup_22.x | bash - >/dev/null
      pkg_install nodejs
      ;;
    zypper) pkg_install nodejs22 npm22 ;;
    pacman) pkg_install nodejs npm ;;
    apk) pkg_install nodejs npm ;;
    *) die "$(t 'установите Node.js 20+ вручную' 'install Node.js 20+ manually')" ;;
  esac
  [[ "$(node_major)" -ge 20 ]] || die "$(t "нужен Node.js 20+, установлен $(node -v 2>/dev/null || echo 'нет')" \
    "Node.js 20+ is required, installed: $(node -v 2>/dev/null || echo 'none')")"
}

build_from_source() { # sets BUNDLE to a freshly assembled bundle directory
  local src="${SOURCE_DIR:-$(source_root_guess)}"
  [[ -n "$src" && -f "$src/Cargo.toml" ]] || die "$(t 'не найдены исходники CrabIndex: укажите --from-source DIR' 'CrabIndex sources not found: pass --from-source DIR')"
  src="$(cd "$src" && pwd)"
  info "$(t "Сборка из исходников: $src" "Building from source: $src")"
  detect_pkg_mgr
  if [[ "$SKIP_DEPS" -eq 0 ]]; then
    command -v cc >/dev/null 2>&1 && command -v pkg-config >/dev/null 2>&1 \
      || {
        local build_pkgs=()
        read -r -a build_pkgs <<<"$(pkg_name build)"
        pkg_install "${build_pkgs[@]}"
      }
    command -v git >/dev/null 2>&1 || pkg_install git
  fi
  ensure_rust
  info "$(t 'Сборка сервера (cargo build --release, несколько минут)' 'Building the server (cargo build --release, a few minutes)')"
  (cd "$src" && "$CARGO_BIN" build --release --locked -p crabindex) || die "$(t 'сборка сервера не удалась' 'server build failed')"
  if [[ ! -f "$src/wwwroot/index.html" || ! -f "$src/wwwroot/admin/index.html" ]]; then
    [[ "$SKIP_DEPS" -eq 0 ]] && ensure_node
    info "$(t 'Сборка веб-интерфейса, админ-панели и документации' 'Building the web UI, admin panel and docs')"
    (cd "$src" && bash scripts/build-web-ui.sh) || die "$(t 'сборка веб-интерфейса не удалась' 'web UI build failed')"
  fi
  WORK_DIR="${WORK_DIR:-$(mktemp -d)}"
  local out="$WORK_DIR/dist"
  mkdir -p "$out/Data"
  cp "$src/target/release/crabindex" "$out/"
  cp -a "$src/wwwroot" "$out/wwwroot"
  cp "$src/Data/example.yaml" "$src/Data/example.conf" "$src/Data/crontab" "$src/Data/run-job.sh" "$out/Data/"
  BUNDLE="$out"
}

# ---------------------------------------------------------------------------
# bundle
# ---------------------------------------------------------------------------

bundle_root_in() { # directory containing the crabindex binary (dir itself or one level down)
  local dir="$1" d
  if [[ -f "$dir/crabindex" ]]; then
    printf '%s' "$dir"
    return 0
  fi
  for d in "$dir"/*/; do
    if [[ -f "${d}crabindex" ]]; then
      printf '%s' "${d%/}"
      return 0
    fi
  done
  return 1
}

release_asset_url() { # URL of the release archive for this machine
  local arch
  case "$(uname -m)" in
    x86_64 | amd64) arch="x86_64" ;;
    aarch64 | arm64) arch="arm64" ;;
    *) die "$(t "нет готовой сборки для архитектуры $(uname -m): используйте --from-source" "no prebuilt release for architecture $(uname -m): use --from-source")" ;;
  esac
  local name="crabindex-linux-${arch}.tar.gz"
  if [[ -n "$RELEASE_TAG" ]]; then
    printf 'https://github.com/%s/releases/download/%s/%s' "$RELEASE_REPO" "$RELEASE_TAG" "$name"
  else
    printf 'https://github.com/%s/releases/latest/download/%s' "$RELEASE_REPO" "$name"
  fi
}

resolve_bundle() { # sets BUNDLE_ROOT (not called in a subshell: WORK_DIR must stay visible)
  local src="$BUNDLE"
  if [[ -z "$src" && -n "$RELEASE_TAG" ]]; then
    src="$(release_asset_url)"
  fi
  if [[ -z "$src" ]]; then
    if [[ -f "$SCRIPT_DIR/crabindex" ]]; then
      src="$SCRIPT_DIR"
    elif [[ -f "$SCRIPT_DIR/../dist/crabindex" ]]; then
      src="$(cd "$SCRIPT_DIR/../dist" && pwd)"
    elif [[ -n "$(source_root_guess)" ]]; then
      info "$(t 'Готового комплекта нет - собираю из исходников' 'No prebuilt bundle - building from source')"
      build_from_source
      src="$BUNDLE"
    else
      src="$(release_asset_url)"
      info "$(t "Скачивание готовой сборки: $src" "Downloading the prebuilt release: $src")"
    fi
  fi
  if [[ "$src" =~ ^https?:// ]]; then
    command -v curl >/dev/null 2>&1 || die "$(t 'для загрузки по URL нужен curl' 'curl is required to download from a URL')"
    WORK_DIR="$(mktemp -d)"
    info "$(t "Загрузка $src" "Downloading $src")"
    curl -fL --retry 3 -o "$WORK_DIR/bundle.tar" "$src" || die "$(t "не удалось скачать $src" "failed to download $src")"
    src="$WORK_DIR/bundle.tar"
  fi
  if [[ -f "$src" ]]; then
    [[ -n "$WORK_DIR" ]] || WORK_DIR="$(mktemp -d)"
    mkdir -p "$WORK_DIR/x"
    tar -xf "$src" -C "$WORK_DIR/x" || die "$(t "не удалось распаковать $src" "failed to unpack $src")"
    src="$WORK_DIR/x"
  fi
  [[ -d "$src" ]] || die "$(t "комплект не найден: $src" "bundle not found: $src")"
  BUNDLE_ROOT="$(bundle_root_in "$src")" || die "$(t "в комплекте нет файла crabindex: $src" "the bundle has no crabindex file: $src")"
}

# ---------------------------------------------------------------------------
# admin credentials
# ---------------------------------------------------------------------------

choose_admin_path() { # choose_admin_path <current> -> path on stdout
  local current="$1" p
  if [[ -n "$ADMIN_PATH_ARG" ]]; then
    p="$(normalize_admin_path "$ADMIN_PATH_ARG")" ||
      die "$(t "--admin-path: недопустимый путь «$ADMIN_PATH_ARG» (один сегмент [a-z0-9_-], 2-32 символа, не зарезервирован)" \
        "--admin-path: invalid path \"$ADMIN_PATH_ARG\" (one segment [a-z0-9_-], 2-32 chars, not reserved)")"
    printf '%s' "$p"
    return
  fi
  if [[ -n "$current" ]] || ! can_prompt; then
    printf '%s' "${current:-/admin}"
    return
  fi
  {
    echo
    t "Путь админ-панели:" "Admin panel path:"
    echo
    t "  1) стандартный /admin" "  1) default /admin"
    echo
    t "  2) своё название" "  2) custom name"
    echo
  } >/dev/tty
  local choice
  while true; do
    choice="$(ask "$(t 'Выберите [1]: ' 'Choose [1]: ')")"
    case "${choice:-1}" in
      1)
        printf '/admin'
        return
        ;;
      2)
        while true; do
          p="$(ask "$(t 'Название (латиница в нижнем регистре, цифры, - и _, 2-32 символа): ' 'Name (lowercase latin letters, digits, - and _, 2-32 chars): ')")"
          if p="$(normalize_admin_path "$p")"; then
            printf '%s' "$p"
            return
          fi
          t "Недопустимый путь. Пример: /my-panel. Нельзя: /api, /cron, /stats, /docs и другие служебные." \
            "Invalid path. Example: /my-panel. Not allowed: /api, /cron, /stats, /docs and other service paths." >/dev/tty
          echo >/dev/tty
        done
        ;;
      *)
        t "Введите 1 или 2." "Enter 1 or 2." >/dev/tty
        echo >/dev/tty
        ;;
    esac
  done
}

prepare_config() { # writes admin.path/admin.token/devkey; prints nothing
  local cfg="$INSTALL_DIR/init.yaml" example="$INSTALL_DIR/Data/example.yaml"
  if [[ ! -f "$cfg" && -f "$INSTALL_DIR/init.conf" ]]; then
    warn "$(t 'используется init.conf (JSON): токен и devkey сгенерирует сервер при первом запуске' \
      'using init.conf (JSON): the server will generate the token and devkey on first start')"
    if [[ "$PORT_WRITE" -eq 1 ]]; then
      warn "$(t "порт $LISTEN_PORT не записан: в init.conf задайте \"listenport\": $LISTEN_PORT вручную" \
        "port $LISTEN_PORT was not saved: set \"listenport\": $LISTEN_PORT in init.conf manually")"
    fi
    CONFIG_IS_JSON=1
    return
  fi
  if [[ ! -f "$cfg" ]]; then
    [[ -f "$example" ]] || die "$(t "нет $example для создания init.yaml" "$example is missing, cannot create init.yaml")"
    install -m 600 -o "$SERVICE_USER" -g "$SERVICE_USER" "$example" "$cfg"
    info "$(t "Создан $cfg из Data/example.yaml" "Created $cfg from Data/example.yaml")"
    CONFIG_CREATED=1
  fi

  local cur_path cur_token cur_devkey path token devkey
  cur_path="$(yaml_admin_get path "$cfg")"
  cur_token="$(yaml_admin_get token "$cfg")"
  cur_devkey="$(yaml_top_get devkey "$cfg")"

  if [[ -n "$cur_path" ]] && ! normalize_admin_path "$cur_path" >/dev/null; then
    warn "$(t "admin.path «$cur_path» в конфиге недопустим - будет выбран заново" "admin.path \"$cur_path\" in the config is invalid - it will be chosen again")"
    cur_path=""
  fi
  # Fresh config from the example: ask for the path even though the example has /admin.
  if [[ -z "$cur_token" && "$MODE" == "install" ]]; then
    cur_path=""
  fi
  path="$(choose_admin_path "$(normalize_admin_path "${cur_path}" 2>/dev/null || true)")"
  token="${cur_token:-$(random_alnum 18)}"
  devkey="${cur_devkey:-$(random_alnum 32)}"

  yaml_admin_set path "\"$path\"" "$cfg"
  [[ -n "$cur_token" ]] || yaml_admin_set token "\"$token\"" "$cfg"
  [[ -n "$cur_devkey" ]] || yaml_top_set devkey "\"$devkey\"" "$cfg"
  if [[ "$PORT_WRITE" -eq 1 ]]; then
    yaml_top_set listenport "$LISTEN_PORT" "$cfg"
  fi
  chown "$SERVICE_USER:$SERVICE_USER" "$cfg"
  chmod 600 "$cfg"
}

print_admin_info() {
  local cfg="$INSTALL_DIR/init.yaml" path token devkey listen
  if [[ "${CONFIG_IS_JSON:-0}" -eq 1 ]]; then
    sleep 3
    echo
    (cd "$INSTALL_DIR" && runuser -u "$SERVICE_USER" -- ./crabindex admin) ||
      warn "$(t "данные входа появятся после первого запуска: cd $INSTALL_DIR && sudo -u $SERVICE_USER ./crabindex admin" \
        "login details will be available after the first start: cd $INSTALL_DIR && sudo -u $SERVICE_USER ./crabindex admin")"
    return
  fi
  path="$(yaml_admin_get path "$cfg")"
  token="$(yaml_admin_get token "$cfg")"
  devkey="$(yaml_top_get devkey "$cfg")"
  listen="$(yaml_top_get listenport "$cfg")"
  local listenip host
  listenip="$(yaml_top_get listenip "$cfg")"
  case "$listenip" in
    127.* | localhost | ::1) host="127.0.0.1" ;;
    "" | any | 0.0.0.0) host="$(server_ip)" ;;
    *) host="$listenip" ;;
  esac
  echo
  echo "════════════════════════════════════════════════════════════"
  echo " $(t 'Админ-панель' 'Admin panel'): http://${host}:${listen:-9117}${path}?${token}"
  if [[ "$host" == "127.0.0.1" ]]; then
    echo " $(t "(сервер слушает только localhost: снаружи - через ваш домен, https://<домен>${path}?${token})" \
      "(the server listens on localhost only: from outside use your domain, https://<domain>${path}?${token})")"
  fi
  echo " $(t 'Пароль' 'Password') (devkey): ${devkey}"
  echo " $(t "Порт: ${listen:-9117} (listenport в $cfg)" "Port: ${listen:-9117} (listenport in $cfg)")"
  local syncapi mode="$CRONTAB_DB_MODE"
  syncapi="$(yaml_top_get syncapi "$cfg")"
  [[ -z "$mode" && -z "$syncapi" ]] && mode="parse"
  case "$mode" in
    sync)
      echo " $(t "База: синхронизация с ${syncapi:-$DB_SYNC_URL}" "Database: sync from ${syncapi:-$DB_SYNC_URL}")"
      echo " $(t 'Перейти на собственный парсинг: install.sh --update --db parse' 'Switch to own parsing later: install.sh --update --db parse')"
      ;;
    parse) echo " $(t 'База: собственный парсинг' 'Database: own parsing')" ;;
  esac
  echo "════════════════════════════════════════════════════════════"
  echo " $(t "Сохраните эти данные. Повторно: cd $INSTALL_DIR && sudo -u $SERVICE_USER ./crabindex admin" \
    "Save these details. To show them again: cd $INSTALL_DIR && sudo -u $SERVICE_USER ./crabindex admin")"
  echo
}

# ---------------------------------------------------------------------------
# Cloudflare bypass: FlareSolverr + cffetch in Docker
# ---------------------------------------------------------------------------

managed_container_exists() { # managed_container_exists <name>
  command -v docker >/dev/null 2>&1 &&
    [[ "$(docker ps -a --filter "name=^/$1\$" --filter "label=$MANAGED_LABEL" -q 2>/dev/null)" != "" ]]
}

# Default FlareSolverr limits scale with the server (FLARESOLVERR_CPUS / FLARESOLVERR_MEMORY
# override them). Memory is what matters: every tracker behind Cloudflare keeps its own Chrome
# tab, and a tab can balloon while solving a challenge. Real case: one site's tab grew to 1.2 GB
# and was OOM-killed ("tab crashed") under a 2.5 GB limit with 4 tracker sessions open; 4 GB
# fixed it.
set_flaresolverr_limits() {
  if [[ -z "$FLARESOLVERR_MEMORY" ]]; then
    local mem_kb
    mem_kb="$(awk '/^MemTotal:/ {print $2}' /proc/meminfo 2>/dev/null || true)"
    [[ "$mem_kb" =~ ^[0-9]+$ ]] || mem_kb=0
    # thresholds in decimal GB (MemTotal in KiB): a "6 GB" VPS reports a bit under 6 GiB
    if ((mem_kb < 5859375)); then
      FLARESOLVERR_MEMORY="1536m"
    elif ((mem_kb < 11718750)); then
      FLARESOLVERR_MEMORY="3g"
    else
      FLARESOLVERR_MEMORY="4g"
    fi
  fi
  if [[ -z "$FLARESOLVERR_CPUS" ]]; then
    local cores
    cores="$(nproc 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1)"
    [[ "$cores" =~ ^[0-9]+$ ]] || cores=1
    if ((cores <= 2)); then
      FLARESOLVERR_CPUS="1"
    elif ((cores <= 5)); then
      FLARESOLVERR_CPUS="1.5"
    else
      FLARESOLVERR_CPUS="2"
    fi
  fi
}

# ---------------------------------------------------------------------------
# database source: sync from sync.crab.rip or own tracker parsing
# ---------------------------------------------------------------------------

# Below 2 cores or 4 GB (decimal) of memory; sets SYS_MEM_GB and SYS_CORES.
weak_system() {
  local mem_kb
  mem_kb="$(awk '/^MemTotal:/ {print $2}' /proc/meminfo 2>/dev/null || true)"
  [[ "$mem_kb" =~ ^[0-9]+$ ]] || mem_kb=0
  SYS_CORES="$(nproc 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1)"
  [[ "$SYS_CORES" =~ ^[0-9]+$ ]] || SYS_CORES=1
  SYS_MEM_GB="$(awk -v k="$mem_kb" 'BEGIN { printf "%.1f", k * 1024 / 1e9 }')"
  (((mem_kb > 0 && mem_kb < 3906250) || SYS_CORES < 2))
}

choose_db_source() { # -> sync|parse on stdout (the question goes to the terminal)
  local choice
  {
    echo
    t "Источник базы раздач:" "Torrent database source:"
    echo
    t "  Собственный парсинг наполняет полную базу несколько дней. Трекеры за Cloudflare" \
      "  Own parsing takes days to fill a full database. Trackers behind Cloudflare"
    echo
    t "  (rutracker, kinozal, selezen и другие) парсятся только через FlareSolverr: он запускает" \
      "  (rutracker, kinozal, selezen and others) can only be parsed with FlareSolverr, which runs"
    echo
    t "  настоящий браузер Chrome и требует минимум 2 ядра CPU и 4 ГБ памяти (рекомендуется" \
      "  a real Chrome browser and needs at least 2 CPU cores and 4 GB of memory (4 cores / 8 GB"
    echo
    t "  4 ядра / 8 ГБ). На слабом сервере собственный парсинг не рекомендуется - выберите синхронизацию." \
      "  recommended). On a weak server own parsing is not recommended - choose sync."
    echo
    if weak_system; then
      t "  Этот сервер слабый: память ${SYS_MEM_GB} ГБ, ядер ${SYS_CORES}." \
        "  This server is weak: ${SYS_MEM_GB} GB of memory, ${SYS_CORES} cores."
      echo
    fi
    echo
    t "  1) Синхронизация с сервером sync.crab.rip (рекомендуется): готовая база скачивается и обновляется сама, минимум нагрузки" \
      "  1) Sync from sync.crab.rip (recommended): a ready database is downloaded and kept up to date, minimal load"
    echo
    t "  2) Собственный парсинг трекеров: база наполняется с нуля, нужны ресурсы и для части трекеров FlareSolverr" \
      "  2) Own tracker parsing: the database is built from scratch, needs resources and FlareSolverr for some trackers"
    echo
  } >/dev/tty
  while true; do
    choice="$(ask "$(t 'Выберите [1]: ' 'Choose [1]: ')")"
    case "${choice//[[:space:]]/}" in
      "" | 1)
        printf 'sync'
        return
        ;;
      2)
        printf 'parse'
        return
        ;;
      *)
        t "Введите 1 или 2." "Enter 1 or 2." >/dev/tty
        echo >/dev/tty
        ;;
    esac
  done
}

current_crontab_is_sync() { # the service user's crontab was installed in sync mode
  command -v crontab >/dev/null 2>&1 &&
    crontab -u "$SERVICE_USER" -l 2>/dev/null | grep -qxF "$DB_SYNC_MARKER"
}

# Sets DB_MODE / DB_APPLY / CRONTAB_DB_MODE. Fresh config: ask (or sync without a terminal);
# existing config without --db: keep syncapi, and a sync-mode crontab stays reduced.
decide_db_source() {
  if [[ -n "$DB_MODE" ]]; then
    DB_APPLY=1
  elif [[ "$CONFIG_CREATED" -eq 1 ]]; then
    DB_APPLY=1
    if can_prompt; then DB_MODE="$(choose_db_source)"; else DB_MODE="sync"; fi
  else
    if current_crontab_is_sync; then CRONTAB_DB_MODE="sync"; fi
    return 0
  fi
  if [[ "$DB_MODE" == parse ]] && weak_system; then
    warn "$(t "слабый сервер (память ${SYS_MEM_GB} ГБ, ядер ${SYS_CORES}): собственный парсинг и FlareSolverr не рекомендуются - нужно минимум 2 ядра и 4 ГБ памяти" \
      "weak server (${SYS_MEM_GB} GB of memory, ${SYS_CORES} cores): own parsing and FlareSolverr are not recommended - at least 2 cores and 4 GB of memory are needed")"
    if can_prompt && ! confirm "$(t 'Продолжить с собственным парсингом?' 'Continue with own parsing?')"; then
      DB_MODE="sync"
    fi
  fi
  CRONTAB_DB_MODE="$DB_MODE"
}

# Writes syncapi for the chosen source (only when DB_APPLY=1).
apply_db_source() {
  [[ "$DB_APPLY" -eq 1 ]] || return 0
  local cfg="$INSTALL_DIR/init.yaml" cur
  if [[ "${CONFIG_IS_JSON:-0}" -eq 1 || ! -f "$cfg" ]]; then
    local val='""'
    [[ "$DB_MODE" == sync ]] && val="\"$DB_SYNC_URL\""
    warn "$(t "syncapi не записан: в init.conf задайте \"syncapi\": $val вручную" \
      "syncapi was not saved: set \"syncapi\": $val in init.conf manually")"
  else
    if [[ "$DB_MODE" == sync ]]; then
      # keep a custom sync server if one is already configured
      cur="$(yaml_top_get syncapi "$cfg")"
      [[ -n "$cur" ]] || yaml_top_set syncapi "$DB_SYNC_URL" "$cfg"
    else
      yaml_top_set syncapi '""' "$cfg"
    fi
    chown "$SERVICE_USER:$SERVICE_USER" "$cfg"
    chmod 600 "$cfg"
  fi
  if [[ "$DB_MODE" == sync ]]; then
    info "$(t 'Источник базы: синхронизация (задания парсинга трекеров не устанавливаются)' 'Database source: sync (tracker parsing jobs are not installed)')"
  else
    info "$(t 'Источник базы: собственный парсинг трекеров' 'Database source: own tracker parsing')"
  fi
}

# Sets WITH_FLARESOLVERR when not given on the command line.
decide_flaresolverr() {
  set_flaresolverr_limits
  # sync mode does not need the Cloudflare bypass unless --flaresolverr asks for it
  if [[ "$DB_APPLY" -eq 1 && "$DB_MODE" == sync ]]; then
    if [[ "$WITH_FLARESOLVERR" == 1 ]]; then
      info "$(t 'FlareSolverr ставится по --flaresolverr, хотя при синхронизации он не нужен' 'Installing FlareSolverr because of --flaresolverr, although sync mode does not need it')"
    else
      WITH_FLARESOLVERR=0
    fi
    return
  fi
  [[ -n "$WITH_FLARESOLVERR" ]] && return
  # update / reinstall: keep whatever is there (unless --update --db parse switches to parsing)
  if { [[ "$MODE" == "update" ]] && ! [[ "$DB_APPLY" -eq 1 && "$DB_MODE" == parse ]]; } ||
    managed_container_exists flaresolverr; then
    WITH_FLARESOLVERR="keep"
    return
  fi
  if ! can_prompt; then
    WITH_FLARESOLVERR=0
    return
  fi
  {
    echo
    if [[ "$UI_LANG" == en ]]; then
      echo "Cloudflare bypass (FlareSolverr + cffetch in Docker)."
      echo "  In own parsing mode it is needed for trackers behind Cloudflare: rutracker, kinozal and others."
      echo "  FlareSolverr runs a real Chrome browser and puts a noticeable load on the server."
      echo "  Container limits based on the server's resources: CPU ${FLARESOLVERR_CPUS}, memory ${FLARESOLVERR_MEMORY}"
      echo "  (to change: FLARESOLVERR_CPUS, FLARESOLVERR_MEMORY)."
      echo "  At least 2 cores and 4 GB of memory are recommended. Without it, trackers behind Cloudflare are not parsed."
    else
      echo "Обход Cloudflare (FlareSolverr + cffetch в Docker)."
      echo "  В режиме собственного парсинга нужен для трекеров за Cloudflare: rutracker, kinozal и других."
      echo "  FlareSolverr запускает настоящий браузер Chrome и заметно нагружает сервер:"
      echo "  Лимиты контейнера по ресурсам сервера: CPU ${FLARESOLVERR_CPUS}, память ${FLARESOLVERR_MEMORY}"
      echo "  (изменить: FLARESOLVERR_CPUS, FLARESOLVERR_MEMORY)."
      echo "  Рекомендуется от 2 ядер и 4 ГБ памяти. Без него закрытые Cloudflare трекеры не парсятся."
    fi
  } >/dev/tty
  if confirm "$(t 'Установить FlareSolverr?' 'Install FlareSolverr?')"; then WITH_FLARESOLVERR=1; else WITH_FLARESOLVERR=0; fi
}

# Points init.yaml at the local containers (1) or turns the bypass off (0).
configure_cf_bypass() { # configure_cf_bypass <1|0>
  local cfg="$INSTALL_DIR/init.yaml"
  [[ -f "$cfg" && "${CONFIG_IS_JSON:-0}" -eq 0 ]] || return 0
  if [[ "$1" -eq 1 ]]; then
    yaml_section_set flaresolverr enable true "$cfg"
    yaml_section_set flaresolverr url "http://127.0.0.1:8191/v1" "$cfg"
    yaml_section_set flaresolverr crawlUrl '""' "$cfg"
    yaml_section_set cffetch enable true "$cfg"
    yaml_section_set cffetch url "http://127.0.0.1:8192/fetch" "$cfg"
    yaml_section_set cffetch proxy '""' "$cfg"
  else
    yaml_section_set flaresolverr enable false "$cfg"
    yaml_section_set cffetch enable false "$cfg"
  fi
  chown "$SERVICE_USER:$SERVICE_USER" "$cfg"
  chmod 600 "$cfg"
}

ensure_docker() {
  if ! command -v docker >/dev/null 2>&1; then
    local pkg=""
    case "$PKG_MGR" in
      apt-get) pkg="docker.io" ;;
      dnf) pkg="moby-engine" ;;
      yum | zypper | pacman | apk) pkg="docker" ;;
    esac
    [[ -n "$pkg" ]] || { warn "$(t 'Docker не найден и не может быть установлен автоматически' 'Docker not found and cannot be installed automatically')"; return 1; }
    pkg_install "$pkg" || { warn "$(t "не удалось установить Docker ($pkg)" "failed to install Docker ($pkg)")"; return 1; }
  fi
  systemctl enable --now docker >/dev/null 2>&1 || true
  docker info >/dev/null 2>&1 || { warn "$(t 'Docker установлен, но не запущен (systemctl status docker)' 'Docker is installed but not running (systemctl status docker)')"; return 1; }
}

run_managed_container() { # run_managed_container <name> <docker run args...>
  local name="$1"
  shift
  if docker ps -a --filter "name=^/${name}\$" -q | grep -q .; then
    if ! managed_container_exists "$name"; then
      warn "$(t "контейнер $name уже есть и создан не установщиком - оставлен как есть" "container $name already exists and was not created by the installer - left as is")"
      return 0
    fi
    docker rm -f "$name" >/dev/null
  fi
  docker run -d --name "$name" --restart unless-stopped --label "$MANAGED_LABEL" "$@" >/dev/null ||
    warn "$(t "не удалось запустить контейнер $name (docker logs $name)" "failed to start container $name (docker logs $name)")"
}

install_flaresolverr() {
  set_flaresolverr_limits
  info "$(t "Установка FlareSolverr и cffetch (Docker, лимиты: FlareSolverr ${FLARESOLVERR_CPUS} CPU / ${FLARESOLVERR_MEMORY}, cffetch ${CFFETCH_CPUS} CPU / ${CFFETCH_MEMORY})" \
    "Installing FlareSolverr and cffetch (Docker, limits: FlareSolverr ${FLARESOLVERR_CPUS} CPU / ${FLARESOLVERR_MEMORY}, cffetch ${CFFETCH_CPUS} CPU / ${CFFETCH_MEMORY})")"
  if ! ensure_docker; then
    warn "$(t 'обход Cloudflare не установлен - трекеры за Cloudflare работать не будут' 'Cloudflare bypass not installed - trackers behind Cloudflare will not work')"
    configure_cf_bypass 0
    return 0
  fi
  # Only on 127.0.0.1: the browser API must never be reachable from outside.
  # shm-size: Chrome crashes or hangs on start with Docker's default 64 MB /dev/shm.
  run_managed_container flaresolverr -p 127.0.0.1:8191:8191 \
    -e LOG_LEVEL=info -e DISABLE_MEDIA=true -e TZ="${TZ:-UTC}" \
    --cpus "$FLARESOLVERR_CPUS" --memory "$FLARESOLVERR_MEMORY" --shm-size 512m \
    "$FLARESOLVERR_IMAGE"
  # cffetch binds 127.0.0.1 inside the container, so it runs on the host network.
  run_managed_container cffetch --network host \
    --cpus "$CFFETCH_CPUS" --memory "$CFFETCH_MEMORY" \
    "$CFFETCH_IMAGE"
  local tries=60
  while ((tries-- > 0)); do
    curl -fsS -m 3 http://127.0.0.1:8191/ >/dev/null 2>&1 && break
    sleep 2
  done
  if curl -fsS -m 3 http://127.0.0.1:8191/ >/dev/null 2>&1; then
    info "$(t 'FlareSolverr запущен (127.0.0.1:8191), cffetch - 127.0.0.1:8192' 'FlareSolverr is running (127.0.0.1:8191), cffetch - 127.0.0.1:8192')"
  else
    warn "$(t 'FlareSolverr не ответил за 2 минуты: docker logs flaresolverr' 'FlareSolverr did not respond within 2 minutes: docker logs flaresolverr')"
  fi
  configure_cf_bypass 1
}

remove_flaresolverr() {
  command -v docker >/dev/null 2>&1 || return 0
  local ids
  ids="$(docker ps -a --filter "label=$MANAGED_LABEL" -q 2>/dev/null)"
  if [[ -n "$ids" ]]; then
    # shellcheck disable=SC2086
    docker rm -f $ids >/dev/null 2>&1 || true
    info "$(t 'Контейнеры FlareSolverr и cffetch удалены' 'FlareSolverr and cffetch containers removed')"
  fi
}

# ---------------------------------------------------------------------------
# install / update / uninstall
# ---------------------------------------------------------------------------

ensure_user() {
  if ! id "$SERVICE_USER" >/dev/null 2>&1; then
    info "$(t "Создание пользователя $SERVICE_USER" "Creating user $SERVICE_USER")"
    local nologin
    nologin="$(command -v nologin || echo /usr/sbin/nologin)"
    useradd --system --home-dir "$INSTALL_DIR" --no-create-home --shell "$nologin" "$SERVICE_USER"
  fi
}

write_unit() {
  info "$(t "Установка systemd unit $UNIT_FILE" "Installing systemd unit $UNIT_FILE")"
  cat >"$UNIT_FILE" <<EOF
[Unit]
Description=CrabIndex torrent aggregator
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=$SERVICE_USER
Group=$SERVICE_USER
WorkingDirectory=$INSTALL_DIR
ExecStart=$INSTALL_DIR/crabindex
Restart=on-failure
RestartSec=5
TimeoutStopSec=60
LimitNOFILE=65536
UMask=0027
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ProtectHome=true
ReadWritePaths=$INSTALL_DIR

[Install]
WantedBy=multi-user.target
EOF
}

copy_bundle() {
  local src="$1"
  info "$(t "Копирование файлов в $INSTALL_DIR" "Copying files to $INSTALL_DIR")"
  mkdir -p "$INSTALL_DIR/Data/fdb" "$INSTALL_DIR/Data/temp" "$INSTALL_DIR/Data/log" "$INSTALL_DIR/Data/tracks"
  install -m 755 "$src/crabindex" "$INSTALL_DIR/crabindex.new"
  mv -f "$INSTALL_DIR/crabindex.new" "$INSTALL_DIR/crabindex"
  if [[ -d "$src/wwwroot" ]]; then
    rm -rf "$INSTALL_DIR/wwwroot.new"
    cp -a "$src/wwwroot" "$INSTALL_DIR/wwwroot.new"
    rm -rf "$INSTALL_DIR/wwwroot"
    mv "$INSTALL_DIR/wwwroot.new" "$INSTALL_DIR/wwwroot"
  else
    warn "$(t 'в комплекте нет wwwroot/ - веб-интерфейс и админ-панель не будут работать' 'the bundle has no wwwroot/ - the web UI and admin panel will not work')"
  fi
  local f
  for f in example.yaml example.conf crontab run-job.sh; do
    if [[ -f "$src/Data/$f" ]]; then
      install -m 644 "$src/Data/$f" "$INSTALL_DIR/Data/$f"
    fi
  done
  [[ -f "$INSTALL_DIR/Data/run-job.sh" ]] && chmod 755 "$INSTALL_DIR/Data/run-job.sh"
  chown "$SERVICE_USER:$SERVICE_USER" "$INSTALL_DIR" "$INSTALL_DIR/Data" "$INSTALL_DIR/crabindex"
  chown -R "$SERVICE_USER:$SERVICE_USER" "$INSTALL_DIR/wwwroot" "$INSTALL_DIR/Data/fdb" "$INSTALL_DIR/Data/temp" \
    "$INSTALL_DIR/Data/log" "$INSTALL_DIR/Data/tracks" 2>/dev/null || true
  chmod 750 "$INSTALL_DIR"
}

# stdin: full Data/crontab -> stdout: only the non-tracker jobs (/jsondb/..., /cron/maintenance/...)
# plus the environment lines, for sync mode.
reduce_crontab() {
  printf '%s\n' "$DB_SYNC_MARKER" \
    "# sync mode: tracker parsing jobs are not installed, the database comes from syncapi." \
    "# Full job list: Data/crontab (install.sh --update --db parse installs it)." ""
  awk '
    /^[[:space:]]*$/ { buf = ""; next }
    /^[[:space:]]*#/ { buf = buf $0 "\n"; next }
    /^[A-Za-z_][A-Za-z0-9_]*=/ { print; sep = 1; buf = ""; next }
    {
      if ($0 ~ /https?:\/\/[^\/ "]+\/(jsondb|cron\/maintenance)\//) {
        if (sep) print ""
        printf "%s", buf
        print
        sep = 1
      }
      buf = ""
    }'
}

install_crontab() {
  local tab="$INSTALL_DIR/Data/crontab"
  if [[ ! -f "$tab" ]]; then
    warn "$(t "нет $tab - crontab не установлен" "$tab is missing - crontab not installed")"
    return
  fi
  if ! command -v crontab >/dev/null 2>&1; then
    warn "$(t 'crontab не найден (установите cron) - задания не установлены' 'crontab not found (install cron) - jobs not installed')"
    return
  fi
  command -v flock >/dev/null 2>&1 || warn "$(t 'flock не найден (пакет util-linux) - нужен для Data/run-job.sh' 'flock not found (util-linux package) - needed by Data/run-job.sh')"
  command -v curl >/dev/null 2>&1 || warn "$(t 'curl не найден - нужен для Data/run-job.sh' 'curl not found - needed by Data/run-job.sh')"
  local path="$INSTALL_DIR/Data/run-job.sh" edits=()
  if [[ "$INSTALL_DIR" != "/opt/crabindex" ]]; then
    edits+=(-e "s|/opt/crabindex/|$INSTALL_DIR/|g")
  fi
  # Data/crontab calls http://127.0.0.1:9117/...; follow a non-default listenport.
  if [[ "$LISTEN_PORT" != 9117 ]]; then
    edits+=(-e "s|127\.0\.0\.1:9117/|127.0.0.1:$LISTEN_PORT/|g")
  fi
  [[ ${#edits[@]} -gt 0 ]] || edits=(-e '')
  if [[ "$CRONTAB_DB_MODE" == sync ]]; then
    reduce_crontab <"$tab" | sed "${edits[@]}" | crontab -u "$SERVICE_USER" -
  else
    sed "${edits[@]}" "$tab" | crontab -u "$SERVICE_USER" -
  fi
  info "$(t "Установлен crontab пользователя $SERVICE_USER ($path, порт $LISTEN_PORT)" "Installed crontab for user $SERVICE_USER ($path, port $LISTEN_PORT)")"
  if [[ "$CRONTAB_DB_MODE" == sync ]]; then
    info "$(t 'Режим синхронизации: в crontab только служебные задания, без парсинга трекеров' 'Sync mode: the crontab has only service jobs, no tracker parsing')"
  fi
}

do_install() {
  require_root
  system_check || die "$(t 'systemd не найден - установка службы невозможна' 'systemd not found - cannot install the service')"
  echo
  ensure_runtime_deps
  [[ "$FROM_SOURCE" -eq 1 ]] && build_from_source
  local existing=0
  [[ -x "$INSTALL_DIR/crabindex" ]] && existing=1
  if [[ "$MODE" == "update" && "$existing" -eq 0 ]]; then
    die "$(t "CrabIndex не установлен в $INSTALL_DIR (для новой установки запустите без --update)" "CrabIndex is not installed in $INSTALL_DIR (run without --update for a fresh install)")"
  fi
  if [[ "$MODE" == "install" && "$existing" -eq 1 ]]; then
    info "$(t "Найдена установка в $INSTALL_DIR - выполняется обновление (конфиг и данные сохраняются)" "Found an installation in $INSTALL_DIR - updating it (config and data are kept)")"
    MODE="update"
  fi
  resolve_listen_port
  resolve_bundle
  local src="$BUNDLE_ROOT"
  info "$(t "Комплект: $src" "Bundle: $src")"

  ensure_user
  if [[ "$MODE" == "update" ]] && systemctl is-active --quiet "$SERVICE_NAME"; then
    info "$(t "Остановка службы $SERVICE_NAME" "Stopping service $SERVICE_NAME")"
    systemctl stop "$SERVICE_NAME"
  fi
  copy_bundle "$src"
  prepare_config
  decide_db_source
  apply_db_source
  decide_flaresolverr
  case "$WITH_FLARESOLVERR" in
    1) install_flaresolverr ;;
    0)
      if [[ "$MODE" == "install" || "$DB_APPLY" -eq 1 ]]; then
        configure_cf_bypass 0
      fi
      ;;
  esac
  write_unit
  install_crontab
  systemctl daemon-reload
  systemctl enable "$SERVICE_NAME" >/dev/null 2>&1
  systemctl restart "$SERVICE_NAME"
  info "$(t "Служба $SERVICE_NAME запущена (журнал: journalctl -u $SERVICE_NAME -f)" "Service $SERVICE_NAME started (log: journalctl -u $SERVICE_NAME -f)")"
  print_admin_info
}

do_uninstall() {
  require_root
  require_systemd
  if [[ "$PURGE" -eq 1 ]] && ! confirm "$(t "Удалить $INSTALL_DIR полностью (база, конфиг, логи) и пользователя $SERVICE_USER?" \
    "Remove $INSTALL_DIR completely (database, config, logs) and user $SERVICE_USER?")"; then
    die "$(t 'отменено' 'cancelled')"
  fi
  if systemctl list-unit-files "${SERVICE_NAME}.service" >/dev/null 2>&1; then
    systemctl disable --now "$SERVICE_NAME" >/dev/null 2>&1 || true
  fi
  rm -f "$UNIT_FILE"
  systemctl daemon-reload
  if id "$SERVICE_USER" >/dev/null 2>&1 && command -v crontab >/dev/null 2>&1; then
    crontab -u "$SERVICE_USER" -r 2>/dev/null || true
  fi
  remove_flaresolverr
  if [[ "$PURGE" -eq 1 ]]; then
    rm -rf "$INSTALL_DIR"
    if id "$SERVICE_USER" >/dev/null 2>&1; then
      userdel "$SERVICE_USER" 2>/dev/null || warn "$(t "не удалось удалить пользователя $SERVICE_USER" "failed to remove user $SERVICE_USER")"
    fi
    info "$(t 'CrabIndex удалён полностью' 'CrabIndex removed completely')"
  else
    rm -rf "$INSTALL_DIR/crabindex" "$INSTALL_DIR/wwwroot"
    info "$(t "CrabIndex удалён; данные и конфиг оставлены в $INSTALL_DIR (--purge - удалить всё)" "CrabIndex removed; data and config are kept in $INSTALL_DIR (--purge removes everything)")"
  fi
}

case "$MODE" in
  check)
    detect_pkg_mgr
    system_check || true
    missing_list="$(missing_runtime_pkgs | tr '\n' ' ')"
    if [[ -n "${missing_list// /}" ]]; then
      echo
      info "$(t "Установщик поставит недостающие пакеты сам: $missing_list" "The installer will install the missing packages itself: $missing_list")"
    fi
    ;;
  uninstall) do_uninstall ;;
  *) do_install ;;
esac
