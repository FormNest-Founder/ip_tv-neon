# NIGHT CITY NEON HUB (ip_tv-neon)

TUI медиа-хаб для IPTV и радио. Rust + ratatui + MPV.

## Возможности

### IPTV
- Категории каналов с подсчётом
- Поиск по каналам в реальном времени (по очищенному имени, без провайдерских префиксов)
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

### Persistent TUI
- MPV запускается без выхода из приложения
- Экран "Now Playing" пока MPV работает
- После закрытия MPV — возврат на предыдущий экран с сохранением позиции
- ESC в TUI — остановка MPV

### Прочее
- Избранное и история (с дедупликацией, лимит 200)
- Настройки: playlist URL, EPG URL, fullscreen, geometry, тема (7 пресетов), очистка истории/избранного
- Локальные плейлисты (.m3u/.m3u8)
- Блоклист стран (латиница + кириллица)
- Бинарный кэш с версионированием

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
# Локальная сборка
cargo build --release
cp target/x86_64-unknown-linux-gnu/release/ip_tv-neon ~/.local/bin/ip_tv

# Удалённая сборка (Oracle VPS, Haswell-optimized)
remote_cargo_build.sh ~/Git/ip_tv-neon ~/.local/bin ip_tv
```

## CLI

```bash
ip_tv            # Запуск TUI
ip_tv --debug    # С дебаг-логом (/tmp/neon_iptv.log)
ip_tv update     # Обновить кэш (плейлист + EPG + радио)
ip_tv diag       # Диагностика кэша
```

## Конфигурация

| Файл | Назначение |
|------|------------|
| `~/.config/neon-iptv/config.json` | Настройки (URL, тема, избранное, история) |
| `~/.cache/neon-iptv/data.bin` | Бинарный кэш данных (bincode) |
| `~/.config/mpv/mpv.conf` | Настройки плеера (MPV сам управляет рендером) |

## Стек

- **Rust** + tokio (async runtime)
- **ratatui** 0.26 + crossterm (TUI)
- **reqwest** (HTTP), **quick-xml** (EPG), **bincode** (кэш)
- **MPV** (внешний плеер, запуск через `Command`)

## Архитектура

```
main.rs    — Event loop, key handling, screen routing
app.rs     — App state, MPV launch, filters, settings logic
ui.rs      — Рендеринг всех экранов (ratatui widgets)
epg.rs     — Загрузка плейлиста, EPG, радио; бинарный кэш
models.rs  — Структуры данных (Config, Channel, EpgProgram, Screen)
utils.rs   — Хелперы (normalize, parse_xml_time, main_log)
consts.rs  — Константы, пути
```
