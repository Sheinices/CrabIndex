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
#   sudo scripts/install.sh [--bundle DIR|FILE|URL | --from-source [DIR]] [--admin-path /x] [--yes]
#   sudo scripts/install.sh --update [--bundle ... | --from-source]
#   sudo scripts/install.sh --uninstall [--purge] [--yes]
#   sudo scripts/install.sh --check
#
# Environment: INSTALL_DIR (default /opt/crabindex), SERVICE_USER (default crabindex).
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
PKG_MGR=""
LISTEN_PORT=9117

info() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mвнимание:\033[0m %s\n' "$*" >&2; }
die() {
  printf '\033[1;31mошибка:\033[0m %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [[ -n "$WORK_DIR" && -d "$WORK_DIR" ]]; then
    rm -rf "$WORK_DIR"
  fi
}
trap cleanup EXIT

usage() {
  cat <<'EOF'
Установка CrabIndex (Linux + systemd).

  sudo install.sh [параметры]

Параметры:
  --bundle PATH|URL   каталог `make dist` или архив .tar.gz/.tar.xz (путь или URL)
                      по умолчанию: каталог скрипта (если в нём есть crabindex) или ../dist
  --from-source [DIR] собрать из исходников (по умолчанию - репозиторий, где лежит скрипт);
                      недостающие gcc, pkg-config, git, Rust (rustup) и Node.js ставятся сами
  --admin-path /x     путь админ-панели без вопросов (один сегмент [a-z0-9_-], 2-32 символа)
  --yes, -y           не задавать вопросов (путь /admin, если не указан --admin-path)
  --update            обновить установленную версию (конфиг и данные сохраняются)
  --uninstall         удалить службу, crontab, бинарник и веб-интерфейс (данные остаются)
  --purge             вместе с --uninstall: удалить также данные, конфиг и пользователя
  --check             только проверить систему и показать, чего не хватает
  --no-deps           не устанавливать системные пакеты (только проверить)
  -h, --help          эта справка

Недостающие пакеты (curl, ca-certificates, tar, xz, gzip, rsync, cron, util-linux/flock,
iproute2) ставятся автоматически через apt, dnf, yum, zypper, pacman или apk.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundle)
      [[ $# -ge 2 ]] || die "--bundle: укажите путь или URL"
      BUNDLE="$2"
      shift 2
      ;;
    --bundle=*)
      BUNDLE="${1#*=}"
      shift
      ;;
    --admin-path)
      [[ $# -ge 2 ]] || die "--admin-path: укажите путь"
      ADMIN_PATH_ARG="$2"
      shift 2
      ;;
    --admin-path=*)
      ADMIN_PATH_ARG="${1#*=}"
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
    --no-deps)
      SKIP_DEPS=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      die "неизвестный параметр: $1"
      ;;
  esac
done

# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------

can_prompt() {
  [[ "$ASSUME_YES" -eq 0 ]] && { : </dev/tty; } 2>/dev/null
}

ask() { # ask <prompt> -> reply on stdout (from the terminal even under `curl | bash`)
  local reply=""
  read -r -p "$1" reply </dev/tty || true
  printf '%s' "$reply"
}

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

# Value of `admin.<key>` from the block-style `admin:` section.
yaml_admin_get() {
  local key="$1" file="$2"
  awk -v key="$key" '
    /^[^[:space:]#][^:]*:/ { in_admin = ($0 ~ /^admin:[[:space:]]*(#.*)?$/); next }
    in_admin && $0 ~ ("^[[:space:]]+" key ":") {
      v = $0
      sub("^[[:space:]]+" key ":", "", v)
      sub(/[[:space:]]+#.*$/, "", v)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", v)
      gsub(/^["\047]|["\047]$/, "", v)
      print v
      exit
    }' "$file"
}

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

yaml_admin_set() { # yaml_admin_set <key> <value> <file>
  local key="$1" value="$2" file="$3"
  awk -v key="$key" -v val="$value" '
    function flush() { if (in_admin && !done) { print "  " key ": " val; done = 1 } }
    /^[^[:space:]#][^:]*:/ {
      flush()
      in_admin = ($0 ~ /^admin:[[:space:]]*(#.*)?$/)
      if (in_admin) seen = 1
      print
      next
    }
    in_admin && !done && $0 ~ ("^[[:space:]]+" key ":") { print "  " key ": " val; done = 1; next }
    { print }
    END {
      flush()
      if (!seen) { print ""; print "admin:"; print "  " key ": " val }
    }' "$file" | replace_content "$file"
}

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

require_root() {
  [[ "${EUID:-$(id -u)}" -eq 0 ]] || die "запустите от root (sudo $0 ...)"
}

require_systemd() {
  command -v systemctl >/dev/null 2>&1 || die "нужен systemd (systemctl не найден)"
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
  [[ -n "$PKG_MGR" ]] || die "не найден менеджер пакетов - установите вручную: $*"
  info "Установка пакетов: $*"
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
      warn "не хватает пакетов (--no-deps): ${missing[*]}"
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
  echo "Проверка системы:"
  command -v systemctl >/dev/null 2>&1 && ok=0 || { ok=1; bad=1; }
  check_line "$ok" "systemd" "нужен для службы crabindex"
  check_line "$([[ -n "$PKG_MGR" ]] && echo 0 || echo 1)" "менеджер пакетов${PKG_MGR:+: $PKG_MGR}" "пакеты придётся ставить вручную"
  for item in "${RUNTIME_DEPS[@]}"; do
    cmd="${item%%:*}"
    [[ "$cmd" == "update-ca-certificates" ]] && continue
    command -v "$cmd" >/dev/null 2>&1 && ok=0 || ok=1
    check_line "$ok" "$cmd" "будет установлен ($(pkg_name "${item#*:}"))"
  done
  local arch
  arch="$(uname -m)"
  case "$arch" in
    x86_64 | aarch64) check_line 0 "архитектура $arch" ;;
    *) check_line 1 "архитектура $arch" "поддерживаются x86_64 и aarch64" ;;
  esac
  local free_gb mem_mb
  free_gb="$(df -Pk "$(dirname "$INSTALL_DIR")" 2>/dev/null | awk 'NR==2 {print int($4/1024/1024)}')"
  if [[ -n "$free_gb" ]]; then
    [[ "$free_gb" -ge 10 ]] && ok=0 || ok=1
    check_line "$ok" "свободно на диске: ${free_gb} ГБ" "для полной базы нужно 10+ ГБ"
  fi
  mem_mb="$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo 2>/dev/null)"
  if [[ -n "$mem_mb" ]]; then
    [[ "$mem_mb" -ge 1024 ]] && ok=0 || ok=1
    check_line "$ok" "память: ${mem_mb} МБ" "рекомендуется 1+ ГБ"
  fi
  if command -v ss >/dev/null 2>&1; then
    if ss -tlnH 2>/dev/null | awk '{print $4}' | grep -qE "[:.]${LISTEN_PORT}\$"; then
      if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
        check_line 0 "порт $LISTEN_PORT занят самим crabindex"
      else
        check_line 1 "порт $LISTEN_PORT занят" "освободите его или смените listenport в init.yaml"
      fi
    else
      check_line 0 "порт $LISTEN_PORT свободен"
    fi
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
    info "Установка Rust (rustup, stable)"
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
  info "Установка Node.js 22 (нужен для сборки веб-интерфейса)"
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
    *) die "установите Node.js 20+ вручную" ;;
  esac
  [[ "$(node_major)" -ge 20 ]] || die "нужен Node.js 20+, установлен $(node -v 2>/dev/null || echo 'нет')"
}

build_from_source() { # sets BUNDLE to a freshly assembled bundle directory
  local src="${SOURCE_DIR:-$(source_root_guess)}"
  [[ -n "$src" && -f "$src/Cargo.toml" ]] || die "не найдены исходники CrabIndex: укажите --from-source DIR"
  src="$(cd "$src" && pwd)"
  info "Сборка из исходников: $src"
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
  info "Сборка сервера (cargo build --release, несколько минут)"
  (cd "$src" && "$CARGO_BIN" build --release --locked -p crabindex) || die "сборка сервера не удалась"
  if [[ ! -f "$src/wwwroot/index.html" || ! -f "$src/wwwroot/admin/index.html" ]]; then
    [[ "$SKIP_DEPS" -eq 0 ]] && ensure_node
    info "Сборка веб-интерфейса, админ-панели и документации"
    (cd "$src" && bash scripts/build-web-ui.sh) || die "сборка веб-интерфейса не удалась"
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

resolve_bundle() { # sets BUNDLE_ROOT (not called in a subshell: WORK_DIR must stay visible)
  local src="$BUNDLE"
  if [[ -z "$src" ]]; then
    if [[ -f "$SCRIPT_DIR/crabindex" ]]; then
      src="$SCRIPT_DIR"
    elif [[ -f "$SCRIPT_DIR/../dist/crabindex" ]]; then
      src="$(cd "$SCRIPT_DIR/../dist" && pwd)"
    elif [[ -n "$(source_root_guess)" ]]; then
      info "Готового комплекта нет - собираю из исходников"
      build_from_source
      src="$BUNDLE"
    else
      die "не найден комплект для установки: укажите --bundle (каталог make dist или архив) или --from-source"
    fi
  fi
  if [[ "$src" =~ ^https?:// ]]; then
    command -v curl >/dev/null 2>&1 || die "для загрузки по URL нужен curl"
    WORK_DIR="$(mktemp -d)"
    info "Загрузка $src"
    curl -fL --retry 3 -o "$WORK_DIR/bundle.tar" "$src" || die "не удалось скачать $src"
    src="$WORK_DIR/bundle.tar"
  fi
  if [[ -f "$src" ]]; then
    [[ -n "$WORK_DIR" ]] || WORK_DIR="$(mktemp -d)"
    mkdir -p "$WORK_DIR/x"
    tar -xf "$src" -C "$WORK_DIR/x" || die "не удалось распаковать $src"
    src="$WORK_DIR/x"
  fi
  [[ -d "$src" ]] || die "комплект не найден: $src"
  BUNDLE_ROOT="$(bundle_root_in "$src")" || die "в комплекте нет файла crabindex: $src"
}

# ---------------------------------------------------------------------------
# admin credentials
# ---------------------------------------------------------------------------

choose_admin_path() { # choose_admin_path <current> -> path on stdout
  local current="$1" p
  if [[ -n "$ADMIN_PATH_ARG" ]]; then
    p="$(normalize_admin_path "$ADMIN_PATH_ARG")" ||
      die "--admin-path: недопустимый путь «$ADMIN_PATH_ARG» (один сегмент [a-z0-9_-], 2-32 символа, не зарезервирован)"
    printf '%s' "$p"
    return
  fi
  if [[ -n "$current" ]] || ! can_prompt; then
    printf '%s' "${current:-/admin}"
    return
  fi
  {
    echo
    echo "Путь админ-панели:"
    echo "  1) стандартный /admin"
    echo "  2) своё название"
  } >/dev/tty
  local choice
  while true; do
    choice="$(ask "Выберите [1]: ")"
    case "${choice:-1}" in
      1)
        printf '/admin'
        return
        ;;
      2)
        while true; do
          p="$(ask "Название (латиница в нижнем регистре, цифры, - и _, 2-32 символа): ")"
          if p="$(normalize_admin_path "$p")"; then
            printf '%s' "$p"
            return
          fi
          echo "Недопустимый путь. Пример: /my-panel. Нельзя: /api, /cron, /stats, /docs и другие служебные." >/dev/tty
        done
        ;;
      *) echo "Введите 1 или 2." >/dev/tty ;;
    esac
  done
}

prepare_config() { # writes admin.path/admin.token/devkey; prints nothing
  local cfg="$INSTALL_DIR/init.yaml" example="$INSTALL_DIR/Data/example.yaml"
  if [[ ! -f "$cfg" && -f "$INSTALL_DIR/init.conf" ]]; then
    warn "используется init.conf (JSON): токен и devkey сгенерирует сервер при первом запуске"
    CONFIG_IS_JSON=1
    return
  fi
  if [[ ! -f "$cfg" ]]; then
    [[ -f "$example" ]] || die "нет $example для создания init.yaml"
    install -m 600 -o "$SERVICE_USER" -g "$SERVICE_USER" "$example" "$cfg"
    info "Создан $cfg из Data/example.yaml"
  fi

  local cur_path cur_token cur_devkey path token devkey
  cur_path="$(yaml_admin_get path "$cfg")"
  cur_token="$(yaml_admin_get token "$cfg")"
  cur_devkey="$(yaml_top_get devkey "$cfg")"

  if [[ -n "$cur_path" ]] && ! normalize_admin_path "$cur_path" >/dev/null; then
    warn "admin.path «$cur_path» в конфиге недопустим - будет выбран заново"
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
  chown "$SERVICE_USER:$SERVICE_USER" "$cfg"
  chmod 600 "$cfg"
}

print_admin_info() {
  local cfg="$INSTALL_DIR/init.yaml" path token devkey listen
  if [[ "${CONFIG_IS_JSON:-0}" -eq 1 ]]; then
    sleep 3
    echo
    (cd "$INSTALL_DIR" && runuser -u "$SERVICE_USER" -- ./crabindex admin) ||
      warn "данные входа появятся после первого запуска: cd $INSTALL_DIR && sudo -u $SERVICE_USER ./crabindex admin"
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
  echo " Админ-панель: http://${host}:${listen:-9117}${path}?${token}"
  if [[ "$host" == "127.0.0.1" ]]; then
    echo " (сервер слушает только localhost: снаружи - через ваш домен, https://<домен>${path}?${token})"
  fi
  echo " Пароль (devkey): ${devkey}"
  echo "════════════════════════════════════════════════════════════"
  echo " Сохраните эти данные. Повторно: cd $INSTALL_DIR && sudo -u $SERVICE_USER ./crabindex admin"
  echo
}

# ---------------------------------------------------------------------------
# install / update / uninstall
# ---------------------------------------------------------------------------

ensure_user() {
  if ! id "$SERVICE_USER" >/dev/null 2>&1; then
    info "Создание пользователя $SERVICE_USER"
    local nologin
    nologin="$(command -v nologin || echo /usr/sbin/nologin)"
    useradd --system --home-dir "$INSTALL_DIR" --no-create-home --shell "$nologin" "$SERVICE_USER"
  fi
}

write_unit() {
  info "Установка systemd unit $UNIT_FILE"
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
  info "Копирование файлов в $INSTALL_DIR"
  mkdir -p "$INSTALL_DIR/Data/fdb" "$INSTALL_DIR/Data/temp" "$INSTALL_DIR/Data/log" "$INSTALL_DIR/Data/tracks"
  install -m 755 "$src/crabindex" "$INSTALL_DIR/crabindex.new"
  mv -f "$INSTALL_DIR/crabindex.new" "$INSTALL_DIR/crabindex"
  if [[ -d "$src/wwwroot" ]]; then
    rm -rf "$INSTALL_DIR/wwwroot.new"
    cp -a "$src/wwwroot" "$INSTALL_DIR/wwwroot.new"
    rm -rf "$INSTALL_DIR/wwwroot"
    mv "$INSTALL_DIR/wwwroot.new" "$INSTALL_DIR/wwwroot"
  else
    warn "в комплекте нет wwwroot/ - веб-интерфейс и админ-панель не будут работать"
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

install_crontab() {
  local tab="$INSTALL_DIR/Data/crontab"
  if [[ ! -f "$tab" ]]; then
    warn "нет $tab - crontab не установлен"
    return
  fi
  if ! command -v crontab >/dev/null 2>&1; then
    warn "crontab не найден (установите cron) - задания не установлены"
    return
  fi
  command -v flock >/dev/null 2>&1 || warn "flock не найден (пакет util-linux) - нужен для Data/run-job.sh"
  command -v curl >/dev/null 2>&1 || warn "curl не найден - нужен для Data/run-job.sh"
  local path="$INSTALL_DIR/Data/run-job.sh"
  if [[ "$INSTALL_DIR" != "/opt/crabindex" ]]; then
    sed "s|/opt/crabindex/|$INSTALL_DIR/|g" "$tab" | crontab -u "$SERVICE_USER" -
  else
    crontab -u "$SERVICE_USER" "$tab"
  fi
  info "Установлен crontab пользователя $SERVICE_USER ($path)"
}

do_install() {
  require_root
  system_check || die "systemd не найден - установка службы невозможна"
  echo
  ensure_runtime_deps
  [[ "$FROM_SOURCE" -eq 1 ]] && build_from_source
  local existing=0
  [[ -x "$INSTALL_DIR/crabindex" ]] && existing=1
  if [[ "$MODE" == "update" && "$existing" -eq 0 ]]; then
    die "CrabIndex не установлен в $INSTALL_DIR (для новой установки запустите без --update)"
  fi
  if [[ "$MODE" == "install" && "$existing" -eq 1 ]]; then
    info "Найдена установка в $INSTALL_DIR - выполняется обновление (конфиг и данные сохраняются)"
    MODE="update"
  fi
  resolve_bundle
  local src="$BUNDLE_ROOT"
  info "Комплект: $src"

  ensure_user
  if [[ "$MODE" == "update" ]] && systemctl is-active --quiet "$SERVICE_NAME"; then
    info "Остановка службы $SERVICE_NAME"
    systemctl stop "$SERVICE_NAME"
  fi
  copy_bundle "$src"
  prepare_config
  write_unit
  install_crontab
  systemctl daemon-reload
  systemctl enable "$SERVICE_NAME" >/dev/null 2>&1
  systemctl restart "$SERVICE_NAME"
  info "Служба $SERVICE_NAME запущена (журнал: journalctl -u $SERVICE_NAME -f)"
  print_admin_info
}

do_uninstall() {
  require_root
  require_systemd
  if [[ "$PURGE" -eq 1 ]] && ! confirm "Удалить $INSTALL_DIR полностью (база, конфиг, логи) и пользователя $SERVICE_USER?"; then
    die "отменено"
  fi
  if systemctl list-unit-files "${SERVICE_NAME}.service" >/dev/null 2>&1; then
    systemctl disable --now "$SERVICE_NAME" >/dev/null 2>&1 || true
  fi
  rm -f "$UNIT_FILE"
  systemctl daemon-reload
  if id "$SERVICE_USER" >/dev/null 2>&1 && command -v crontab >/dev/null 2>&1; then
    crontab -u "$SERVICE_USER" -r 2>/dev/null || true
  fi
  if [[ "$PURGE" -eq 1 ]]; then
    rm -rf "$INSTALL_DIR"
    if id "$SERVICE_USER" >/dev/null 2>&1; then
      userdel "$SERVICE_USER" 2>/dev/null || warn "не удалось удалить пользователя $SERVICE_USER"
    fi
    info "CrabIndex удалён полностью"
  else
    rm -rf "$INSTALL_DIR/crabindex" "$INSTALL_DIR/wwwroot"
    info "CrabIndex удалён; данные и конфиг оставлены в $INSTALL_DIR (--purge - удалить всё)"
  fi
}

case "$MODE" in
  check)
    detect_pkg_mgr
    system_check || true
    missing_list="$(missing_runtime_pkgs | tr '\n' ' ')"
    if [[ -n "${missing_list// /}" ]]; then
      echo
      info "Установщик поставит недостающие пакеты сам: $missing_list"
    fi
    ;;
  uninstall) do_uninstall ;;
  *) do_install ;;
esac
