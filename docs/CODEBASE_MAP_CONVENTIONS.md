---
parent: CODEBASE_MAP.md
last_mapped: 2026-07-13T14:38:00Z
section: conventions
---

> Part of [Codebase Map](CODEBASE_MAP.md)

# Conventions & Standards

## Rust (Backend) Conventions

### Code Style

| Aspect | Convention | Tool |
|--------|------------|------|
| Formatter | `rustfmt` | Default rustfmt.toml |
| Linter | `clippy` | `cargo clippy -- -D warnings` |
| Edition | Rust 2021 | Cargo.toml |
| Line width | 100 chars | rustfmt default |

### Naming Conventions

| Element | Convention | Example |
|---------|------------|---------|
| Modules | `snake_case` | `whisper_engine`, `audio_device` |
| Functions | `snake_case` | `transcribe_audio()`, `load_model()` |
| Types/Structs | `PascalCase` | `WhisperEngine`, `AudioDevice` |
| Constants | `SCREAMING_SNAKE_CASE` | `DEFAULT_MODEL_SIZE`, `MAX_CHUNKS` |
| Environment vars | `UPPER_SNAKE_CASE` | `WHISPER_MODEL_PATH`, `OLLAMA_BASE_URL` |

### Module Organization

```
src/
├── main.rs              # Entry point, Tauri build
├── lib.rs               # Library root, command registration
└── {module}/            # Feature module directory
    ├── mod.rs           # Module root, re-exports
    ├── {module}.rs      # Core implementation
    ├── commands.rs      # Tauri command handlers
    └── types.rs         # Shared type definitions (optional)
```

### Error Handling Pattern

```rust
// Use Result<T, String> for Tauri commands
#[tauri::command]
fn transcribe_audio(path: String) -> Result<TranscriptionResult, String> {
    // Return descriptive error messages
    Ok(result)
}

// Use custom error types internally
enum AppError {
    Audio(String),
    Transcription(String),
    Database(String),
}
```

### Async Pattern

```rust
// Spawn blocking operations
tokio::spawn(async move {
    // Long-running work
});

// Share state with Arc
let shared_state = Arc::new(AppState { ... });
```

## TypeScript/React (Frontend) Conventions

### Code Style

| Aspect | Convention | Tool |
|--------|------------|------|
| Formatter | Prettier | `prettier` config in pnpm-lock.yaml |
| Linter | ESLint | `eslint.config.mjs` |
| Language | TypeScript strict | `tsconfig.json` |
| Import style | Absolute from `@/` | `tsconfig.paths` |

### Naming Conventions

| Element | Convention | Example |
|---------|------------|---------|
| Components | `PascalCase` | `RecordingControls`, `MeetingCard` |
| Hooks | `useCamelCase` | `useAudioDevices`, `useRecording` |
| Functions | `camelCase` | `startRecording()`, `getMeetings()` |
| Interfaces/Types | `PascalCase` | `AudioDevice`, `MeetingEntry` |
| Constants | `UPPER_SNAKE_CASE` | `MAX_RECORDING_DURATION` |

### Component Pattern

```tsx
// Functional component with typed props
interface MyComponentProps {
  title: string;
  onClick?: () => void;
}

const MyComponent: React.FC<MyComponentProps> = ({ title, onClick }) => {
  // Implementation
};

export default MyComponent;
```

### State Management Pattern

```typescript
// Zustand store pattern
interface AudioStore {
  devices: AudioDevice[];
  selectedDeviceId: string | null;
  setDevices: (devices: AudioDevice[]) => void;
  setSelectedDevice: (id: string) => void;
}

export const useAudioStore = create<AudioStore>((set) => ({
  devices: [],
  selectedDeviceId: null,
  setDevices: (devices) => set({ devices }),
  setSelectedDevice: (id) => set({ selectedDeviceId: id }),
}));
```

### File Organization

```
frontend/src/
├── app/                   # Next.js App Router pages
│   ├── layout.tsx         # Root layout
│   ├── page.tsx           # Home page
│   └── ...
├── components/            # Reusable UI components
│   ├── ui/                # Shadcn/ui primitives
│   ├── features/          # Feature-specific components
│   └── icons/             # Custom SVG icons
├── hooks/                 # Custom React hooks
├── lib/                   # Utilities and config
├── stores/                # Zustand stores
└── types/                 # TypeScript type definitions
```

## Python (Backend Server) Conventions

### Code Style

| Aspect | Convention | Tool |
|--------|------------|------|
| Formatter | Black | `black .` in backend/ |
| Linter | Flake8 / Ruff | `ruff check .` |
| Type hints | Preferred | `mypy` optional |

### Naming Conventions

| Element | Convention | Example |
|---------|------------|---------|
| Modules/Packages | `snake_case` | `whisper_server`, `audio_processor` |
| Functions/Methods | `snake_case` | `transcribe_file()`, `load_model()` |
| Classes | `PascalCase` | `WhisperServer`, `AudioProcessor` |
| Constants | `UPPER_SNAKE_CASE` | `DEFAULT_SAMPLE_RATE` |

### Requirements File

```txt
# backend/requirements.txt
whisper-rs==...
ollama==...
fastapi==...
uvicorn==...
```

## Git Conventions

### Branch Naming

| Type | Pattern | Example |
|------|---------|---------|
| Feature | `feature/description` | `feature/gpu-acceleration` |
| Bug fix | `fix/description` | `fix/audio-device-detection` |
| Release | `release/version` | `release/v1.2.0` |

### Commit Message Format

```
<type>(<scope>): <subject>

<body> (optional)
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`

Example:
```
feat(audio): add device selection dialog

- Add device selector component
- Integrate with useAudioDevices hook
- Update settings page UI
```

## File Naming Conventions

| Context | Pattern | Example |
|---------|---------|---------|
| Rust modules | `snake_case.rs` | `whisper_engine.rs` |
| TypeScript components | `PascalCase.tsx` | `RecordingControls.tsx` |
| Python modules | `snake_case.py` | `audio_processor.py` |
| Config files | `kebab-case` or `.env` | `.env.example`, `tsconfig.json` |

## Documentation Conventions

### Rust Doc Comments

```rust
/// Brief description of function.
///
/// More detailed explanation if needed.
///
/// # Arguments
/// * `param_name` - Description
///
/// # Returns
/// Description of return value
fn my_function(param: String) -> Result<(), String> { ... }
```

### TypeScript JSDoc

```typescript
/**
 * Brief description of function.
 *
 * @param param1 Description
 * @returns Description of return value
 */
function myFunction(param1: string): Promise<void> { ... }
```

## Environment Variables Convention

| Prefix | Purpose | Example |
|--------|---------|---------|
| `APP_` | App configuration | `APP_NAME`, `APP_VERSION` |
| `WHISPER_` | Whisper engine config | `WHISPER_MODEL_PATH`, `WHISPER_GPU_DEVICE` |
| `OLLAMA_` | Ollama LLM config | `OLLAMA_BASE_URL`, `OLLAMA_MODEL_NAME` |
| `AUDIO_` | Audio module config | `AUDIO_SAMPLE_RATE`, `AUDIO_DEVICE_ID` |