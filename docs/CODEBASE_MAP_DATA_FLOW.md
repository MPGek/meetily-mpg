---
parent: CODEBASE_MAP.md
last_mapped: 2026-07-13T14:37:00Z
section: data_flow
---

> Part of [Codebase Map](CODEBASE_MAP.md)

# Data Flow Diagrams

## Recording and Transcription Pipeline

```mermaid
graph TB
    subgraph Frontend
        UI[Recording UI]
        Devices[Device Selector]
        LiveTranscript[Live Transcript Display]
    end

    subgraph TauriLayer
        Invoke[Tauri invoke]
        Events[Tauri events]
    end

    subgraph RustBackend
        AudioEngine[Audio Engine Module]
        Chunker[Chunk Processor]
        Whisper[Whisper/Parakeet Engine]
        DB[(SQLite Database)]
    end

    subgraph PythonBackend
        Ollama[Ollama Local LLM]
        CloudAPI[Cloud API]
    end

    subgraph Storage
        Recordings[Audio Files]
        Transcripts[Transcript Text]
        Summaries[AI Summaries]
    end

    UI --> Invoke
    Devices --> Invoke
    Invoke --> AudioEngine
    AudioEngine --> Chunker
    Chunker --> Whisper
    Whisper --> LiveTranscript
    LiveTranscript --> Events
    Events --> LiveTranscript
    
    Whisper --> DB
    DB --> Transcripts
    DB --> Recordings
    
    DB -.->|transcript text| SummaryFlow[Summary Engine]
    SummaryFlow --> Ollama
    SummaryFlow --> CloudAPI
    Ollama --> Summaries
    CloudAPI --> Summaries
```

### Flow Description

1. **User selects audio device** → Tauri invoke → Rust AudioEngine
2. **Recording starts** → AudioEngine captures audio stream → chunks sent to disk
3. **Chunks processed** → Whisper/Parakeet transcribes each chunk in real-time
4. **Transcript results** → Tauri event → Frontend LiveTranscript component updates
5. **Recording stops** → AudioEngine finalizes file → saved to Recordings

## Summary Generation Flow

```mermaid
graph TB
    Transcript[Transcript Text] --> DB[(SQLite)]
    DB --> UserClick["User Clicks Summarize"]
    UserClick --> Frontend[Frontend Component]
    Frontend --> TauriInvoke[Tauri invoke: summarize_meeting]
    TauriInvoke --> SummaryService[Summary Service]
    SummaryService --> LangDetect[Language Detection]
    LangDetect --> Template[Prompt Template]
    Template --> ProviderSelect{Provider?}
    ProviderSelect -->|Ollama| OllamaClient[Ollama Client]
    ProviderSelect -->|OpenAI| OpenAIClient[OpenAI Client]
    ProviderSelect -->|Anthropic| AnthropicClient[Anthropic Client]
    
    OllamaClient --> LocalLLM[Local LLM API]
    OpenAIClient --> CloudAPI[Cloud API]
    AnthropicClient --> CloudAPI
    
    CloudAPI --> Response[LLM Response]
    LocalLLM --> Response
    Response --> Parse[Parse and Validate]
    Parse --> SaveDB[(Save to DB)]
    SaveDB --> FrontendUpdate[Frontend Update]
```


## Meeting Data Lifecycle

```mermaid
stateDiagram-v2
    [*] --> Created: User creates meeting
    Created --> Recording: Start recording
    Recording --> Transcribing: Auto-transcribe
    Transcribing --> TranscriptSaved: Stop recording
    TranscriptSaved --> SummaryPending: User requests summary
    SummaryPending --> SummaryGenerated: AI completes
    SummaryGenerated --> Archived: Meeting stored
    
    TranscriptSaved --> TranscriptEdited: User edits transcript
    TranscriptEdited --> TranscriptSaved
    
    SummaryGenerated --> SummaryUpdated: User updates summary
    SummaryUpdated --> SummaryGenerated
    
    Archived --> Deleted: User deletes meeting
    Archived --> Exported: User exports data
```

## Configuration Flow

```mermaid
graph LR
    ConfigFile[Config File<br/>.env / tauri.conf.json] --> Load[App Load]
    Load --> SettingsUI[Settings UI]
    SettingsUI --> UserChange[User Changes Setting]
    UserChange --> Validate{Valid?}
    Validate -->|Yes| Save[Save to Config]
    Validate -->|No| ShowError[Show Error Toast]
    Save --> Apply[Apply to Runtime]
    Apply --> RestartRequired{Needs Restart?}
    RestartRequired -->|Yes| ShowRestart[Show Restart Prompt]
    RestartRequired -->|No| Ready[Ready]
    ShowRestart --> RestartApp[User Restarts App]
    RestartApp --> Ready
```


## Tauri Command Flow (Frontend to Backend)

```mermaid
graph TB
    subgraph ReactUI
        Button[Button Click]
        Hook[Custom Hook]
        Store[Zustand Store]
    end
    
    subgraph IPCBridge
        Invoke[@tauri-apps/api invoke]
        RustCmd[Rust Tauri Command]
    end
    
    subgraph RustModule
        Handler[Command Handler]
        Logic[Business Logic]
        DB[(SQLite)]
    end
    
    Button --> Hook
    Hook --> Invoke
    Store --> Invoke
    Invoke --> RustCmd
    RustCmd --> Handler
    Handler --> Logic
    Logic --> DB
    Logic --> Response[Response Data]
    Response --> IPCBridge
    IPCBridge --> ReactUI
```


## Audio Device Discovery Flow

```mermaid
graph TB
    AppStart[App Start] --> EnumDevices[Tauri: get_audio_devices]
    EnumDevices --> RustBackend[Rust: audio module]
    RustBackend --> PortAudio[PortAudio API]
    PortAudio --> Devices[Enumerate Devices]
    Devices --> FilterChoice{Filter?}
    FilterChoice -->|Yes| Filtered[Apply Filters<br/>input/output/default]
    FilterChoice -->|No| AllDevices[All Devices]
    Filtered --> TauriEvent[Tauri Event: devices_updated]
    AllDevices --> TauriEvent
    TauriEvent --> ReactHook[useAudioDevices hook]
    ReactHook --> Zustand[Zustand Store Update]
    Zustand --> DeviceSelector[Device Selector UI]
```

## Key Data Paths Summary

| Path | Direction | Technology | Purpose |
|------|-----------|------------|---------|
| Recording start/stop | Frontend → Backend | Tauri invoke | Control recording lifecycle |
| Live transcript | Backend → Frontend | Tauri event | Real-time transcription display |
| Meeting list | Backend → Frontend | Tauri invoke + SQL query | Display meetings in UI |
| Summary generation | Frontend → Backend → API | HTTP + Tauri | AI-powered summarization |
| Settings changes | Frontend → Config file | File write | Persist user preferences |
| Device enumeration | Backend → Frontend | Tauri event | Audio device selection |