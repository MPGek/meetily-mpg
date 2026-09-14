## Why

Список Meeting Notes показывает дату записи, но под ней сегодня лежит `meetings.created_at` — время остановки записи (строка БД создается при сохранении на стопе), а не время старта. Для длинных встреч расхождение — часы, и «когда была встреча» отвечает неверно. При этом настоящее время старта уже существует — `metadata.json` пишет `created_at` на старте записи — но в БД никогда не попадает.

## What Changes

- В `meetings` добавляется nullable-колонка `started_at` (время начала записи, UTC RFC3339).
- При сохранении обычной записи `started_at` берется из `metadata.json created_at` папки встречи (фолбэк — текущее время при отсутствии/порче файла).
- При импорте аудио `started_at` берется из mtime аудиофайла (фолбэк — текущее время).
- Существующие строки backfill'ятся `started_at = created_at` (честно документируется: у старых встреч там время стопа).
- API списка встреч отдает `started_at`; отображение предпочитает `started_at ?? created_at`.
- Порядок списка (`created_at DESC`) и семантика `created_at`/`updated_at` не меняются.

## Capabilities

### New Capabilities

- `recording-start-time`: персистентность времени старта записи (источники для записи/импорта/фолбэки), отдача в API списка, правило отображения `started_at ?? created_at`.

### Modified Capabilities

- `database`: новая колонка `meetings.started_at` (nullable, backfill), учет в путях вставки (`save_transcript`, импорт).

## Impact

- Rust: SQLx-миграция (+ backfill UPDATE), `MeetingModel` + DTO списка (`Meeting`, `MeetingMetadata` — по необходимости), чтение `metadata.json` при сохранении (переиспользовать `summary/metadata.rs`), mtime файла при импорте.
- Frontend: типы встреч (`started_at?`), правило `started_at ?? created_at` в `formatMeetingDate`-потребителях (сайдбар; позже — везде, где показывается дата встречи).
- Обратная совместимость: колонка nullable, старые клиенты/строки работают; фолбэк гарантирует дату всегда.
