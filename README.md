# NIGHT CITY NEON HUB (ip_tv-neon)

TUI медиа-хаб для IPTV и радио. Rust + ratatui + MPV.

## Возможности

### IPTV
- Категории каналов с предвычисленным подсчётом (O(1) на рендер)
- Поиск по каналам в реальном времени (предвычисленный lowercase, без аллокаций на keypress)
- EPG: прогрессбар текущей передачи с градиентом (зелёный/жёлтый/оранжевый), название программы
- Маркеры каналов: `★` избранное, `⏪` архив
- Detail-экран: полная EPG-программа с временами, описаниями, выделением текущей передачи

### TimeShift (Архив)
- Автоопределение `tvg-rec` из плейлиста (дни архива)
- Прошлые передачи воспроизводятся через catchup URL (`?utc=START&lutc=STOP`)
- В Detail-экране архивные программы помечены `⏪`

### Radio Record
- Все станции Radio Record с жанрами
- Текущий трек (исполнитель — песня) в списке станций
- Неблокирующий fetch треков через `tokio::spawn`

### Suspended Mode (Video)
- При запуске видео TUI окно скрывается (niri IPC → workspace 4)
- После закрытия MPV — TUI автоматически возвращается (workspace 1)
- Защитный timeout 12ч на ожидание MPV
- Radio: фоновый процесс, TUI остаётся видимым

### Прочее
- Избранное и история (с дедупликацией, лимит 200)
- Настройки: playlist URL, EPG URL, fullscreen, geometry, тема (7 пресетов), очистка истории/избранного
- Локальные плейлисты (.m3u/.m3u8 из ~/Downloads, ~/Videos)
- Бинарный кэш с версионированием (авто-сброс при обновлении)
- Panic hook: терминал корректно восстанавливается при падении

## Управление

| Клавиша | Действие |
|---------|----------|
| `↑/↓` | Навигация |
| `Enter` | Выбор / воспроизведение |
| `Esc` | Назад / остановить MPV |
| `f` | Добавить/убрать из избранного |
| `l` | Live (в Detail-экране) |
| Буквы | Поиск (в списке каналов) |
| `Backspace` | Стереть символ поиска |
| `Ctrl+C` | Выход |

## Сборка и установка

```bash
# Удалённая сборка (Oracle VPS Docker, Haswell-optimized, Clang LTO)
remote_cargo_build.sh ~/Git/ip_tv-neon ~/.local/bin ip_tv
```

## CLI

```bash
ip_tv            # Запуск TUI
ip_tv --debug    # С дебаг-логом (/tmp/neon_iptv.log)
ip_tv update     # Обновить кэш (плейлист + EPG + радио)
ip_tv diag       # Диагностика (пути, URLs, состояние кэша)
```

## Конфигурация

| Файл | Назначение |
|------|------------|
| `~/.config/neon-iptv/config.json` | Настройки (URL, тема, избранное, история) |
| `~/.cache/neon-iptv/data.bin` | Бинарный кэш данных (bincode, версия 911) |
| `~/.config/mpv/mpv.conf` | Настройки плеера |

## Стек

- **Rust** + tokio (async: process, spawn, timeout)
- **ratatui** 0.28 + crossterm (TUI)
- **reqwest** (HTTP), **quick-xml** (EPG, enum-based parser), **bincode** (кэш)
- **MPV** (внешний плеер, `tokio::process::Command`, setsid)
- **niri IPC** (управление окнами в Wayland)

## Архитектура

```
main.rs    — Event loop, async radio fetch, panic hook, suspended mode
app.rs     — App state, MPV launch (video/radio), filters, sorted_favorites
ui.rs      — Рендеринг всех экранов (ratatui widgets, group_counts O(1))
epg.rs     — Плейлист/EPG/радио парсинг, enum XmlTag, shared HTTP client
models.rs  — Структуры (Config, Channel+name_lower, Screen Copy, AppData+group_counts)
utils.rs   — normalize, parse_xml_time (NaiveDateTime fallback), main_log
consts.rs  — Константы, XDG пути, версия кэша
```
