# summary-service Specification

## Purpose
TBD - created by archiving change collect-spec. Update Purpose after archive.
## Requirements
### Requirement: Multi-provider LLM integration
The system SHALL support summarization via 5+ LLM providers: Ollama (local), OpenAI, Anthropic Claude, Groq, and CustomOpenAI (any OpenAI-compatible endpoint).

#### Scenario: Generate summary using Ollama local model
- **WHEN** user selects "Ollama" provider with a local model like "gemma3:1b"
- **THEN** system sends the transcript to the configured Ollama endpoint and returns a markdown summary

#### Scenario: Generate summary using OpenAI
- **WHEN** user selects "OpenAI" provider with API key configured
- **THEN** system sends the transcript to OpenAI's API and returns a markdown summary

### Requirement: Transcript chunking for large inputs
The system SHALL split transcripts into chunks that fit within the model's token threshold when the full text exceeds available context.

#### Scenario: Chunk 50K-word transcript for Ollama
- **WHEN** transcript text exceeds the available token budget (context_size - 300)
- **THEN** system splits into sequential chunks and processes each, combining results

### Requirement: Meeting summary template support
The system SHALL apply structured templates to format summaries with defined sections (e.g., Action Items, Decisions, Key Points).

#### Scenario: Apply "daily_standup" template
- **WHEN** user selects the daily standup template for a meeting summary
- **THEN** system generates a summary formatted as Status Updates, Blockers, and Next Steps

### Requirement: Summary caching with content fingerprinting
The system SHALL cache summaries keyed by transcript text hash + prompt hash + template ID + model config. Changing any input invalidates the cache.

#### Scenario: Reuse cached English summary for translation
- **WHEN** user generates an English summary then switches to German target language
- **THEN** system reuses the cached English markdown and only translates, avoiding a second LLM call

### Requirement: Summary cancellation support
The system SHALL allow cancelling an in-progress summary generation via CancellationToken.

#### Scenario: Cancel long-running summary
- **WHEN** user clicks "Cancel" during summary generation
- **THEN** system sets the cancellation token and stops further LLM calls, updating DB status to cancelled

### Requirement: Language detection for summary target
The system SHALL detect the transcript's source language and offer translation to a configured output language.

#### Scenario: Detect French transcript and translate to English
- **WHEN** user generates a summary with "auto-translate" enabled on a French transcript
- **THEN** system detects French, translates the LLM output to English markdown

### Requirement: Built-in AI model registry
The system SHALL provide a built-in catalog of recommended models (Ollama, OpenAI, Anthropic, Groq) with context sizes and recommendations.

#### Scenario: Get recommended summary model
- **WHEN** user opens settings and views "Recommended Models" section
- **THEN** system displays curated list with accuracy/speed ratings and suggested use cases

