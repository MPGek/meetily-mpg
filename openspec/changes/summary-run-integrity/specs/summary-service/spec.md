## MODIFIED Requirements

### Requirement: Transcript chunking for large inputs
The system SHALL split transcripts into chunks that fit within the model's token threshold when the full text exceeds available context. Each chunk's output SHALL be cleaned of model reasoning before it is combined. A chunk that fails SHALL be retried once; if it fails again, the whole summary run SHALL fail with a user-visible error and no partial summary SHALL be published.

#### Scenario: Chunk 50K-word transcript for Ollama
- **WHEN** transcript text exceeds the available token budget (context_size - 300)
- **THEN** system splits into sequential chunks and processes each, combining results

#### Scenario: Chunk reasoning never reaches the combine step
- **WHEN** a chunk's model output contains a `<think>…</think>` block followed by visible summary text
- **THEN** only the visible summary text of that chunk is included in the input to the combine step

#### Scenario: Failed chunk succeeds on retry
- **WHEN** a chunk's first attempt fails (a request error, or output with no visible content after cleaning) and its second attempt succeeds
- **THEN** the run continues with that chunk's second output and completes normally

#### Scenario: Chunk fails twice
- **WHEN** a chunk fails on both its first attempt and its retry
- **THEN** the run ends as failed with an error that names the failed section (e.g. "transcript section 3 of 7") and asks the user to retry
- **AND** no combine or final-report call is made for that run
- **AND** the meeting's previously saved summary, if any, remains the displayed summary

#### Scenario: Cancellation is not retried
- **WHEN** the user cancels while a chunk is being processed
- **THEN** the chunk is not retried and the run ends as cancelled, not failed

### Requirement: Summary cancellation support
The system SHALL allow cancelling an in-progress summary generation via CancellationToken. Cancellation SHALL be scoped to a single run of a meeting: a cancel request names the run it targets and SHALL NOT affect any other run of the same meeting.

#### Scenario: Cancel long-running summary
- **WHEN** user clicks "Cancel" during summary generation
- **THEN** system sets the cancellation token and stops further LLM calls, updating DB status to cancelled

#### Scenario: Cancel naming a superseded run
- **WHEN** a cancel request names a run that has already been replaced by a newer run of the same meeting
- **THEN** the newer run keeps running and its stored status is unchanged
- **AND** the cancel response reports that no active generation was cancelled

#### Scenario: Starting a new run supersedes the old one
- **WHEN** a new summary run starts for a meeting whose previous run is still in progress
- **THEN** the previous run is cancelled and can no longer change the meeting's stored summary state

#### Scenario: Cancel immediately after start
- **WHEN** the user cancels a run right after starting it, before the run has made any LLM call
- **THEN** the run is cancelled and makes no LLM call

## ADDED Requirements

### Requirement: Summary output excludes model reasoning
Every LLM stage of a summary run (chunk, combine, final report, translation, English normalization) SHALL remove reasoning envelopes (`<think>`/`<thinking>`, any letter case, with or without attributes) from the model output. Separately returned reasoning fields SHALL NOT be used as summary text. Model reasoning text SHALL never be saved as part of a summary.

#### Scenario: Attributed, mixed-case reasoning block is removed
- **WHEN** a stage returns `Intro <THINKING class="internal">private</THINKING> # Meeting`
- **THEN** the stage's cleaned output contains `Intro` and `# Meeting` and does not contain `private`

#### Scenario: Look-alike tag is kept
- **WHEN** a stage returns `<thinker>Visible</thinker>`
- **THEN** the cleaned output is `<thinker>Visible</thinker>` unchanged

#### Scenario: Reasoning returned in a separate field
- **WHEN** an OpenAI-compatible provider returns a message with `content` holding the summary and `reasoning_content` holding reasoning
- **THEN** the stage output is the `content` text only

#### Scenario: Null content with reasoning only
- **WHEN** an OpenAI-compatible provider returns a message with `"content": null` and a non-empty `reasoning` field
- **THEN** the response is parsed without error and the stage output is empty, which fails the stage per the visible-content requirement

### Requirement: Summary stages require visible content
A summary stage SHALL fail when its cleaned output is empty, or when it still contains an unclosed or stray reasoning marker. A failed final-report or translation stage SHALL fail the run with a user-visible error. A failed English-normalization stage SHALL keep the existing fallback to the pre-normalization English summary.

#### Scenario: Unclosed reasoning marker
- **WHEN** the final-report stage returns `Visible text\n<think>private` (no closing tag)
- **THEN** the run fails with an error stating that the output contained an unterminated reasoning marker, and no summary is saved as completed

#### Scenario: Reasoning-only output
- **WHEN** the final-report stage returns only `<think>…</think>` with nothing outside it
- **THEN** the run fails with an error stating that no visible summary content remained after reasoning removal

#### Scenario: Empty normalization output falls back
- **WHEN** the English-normalization stage returns no visible content
- **THEN** the run completes with the pre-normalization English summary

### Requirement: Ollama reasoning is disabled with a compatibility fallback
Requests to Ollama's OpenAI-compatible chat endpoint SHALL ask for reasoning to be disabled (`reasoning_effort: "none"`). If Ollama rejects that field with HTTP 400 or 422, the system SHALL send the request once more without the field. Requests to other providers SHALL NOT include the field.

#### Scenario: Ollama accepts the field
- **WHEN** a summary stage calls Ollama and Ollama accepts `reasoning_effort: "none"`
- **THEN** exactly one request is sent for that stage and it carries `reasoning_effort: "none"`

#### Scenario: Older Ollama rejects the field
- **WHEN** Ollama answers the first request with HTTP 400 and an error body that refers to `reasoning_effort` or thinking
- **THEN** the system sends one more request with the same messages and without `reasoning_effort`, and uses that response

#### Scenario: Unrelated 400 is surfaced
- **WHEN** Ollama answers with HTTP 400 for a reason that does not mention `reasoning_effort` or thinking (e.g. an unknown model)
- **THEN** the error is surfaced without a second request

#### Scenario: Other providers are unchanged
- **WHEN** a summary stage calls OpenAI, Groq, OpenRouter, or a custom OpenAI-compatible endpoint
- **THEN** the request body does not contain `reasoning_effort`

### Requirement: Only the current run determines a meeting's summary state
For each meeting, the most recently started summary run SHALL be the only run whose completion, failure, or cancellation changes the meeting's stored summary status and result. A run's side effects on completion, including renaming the meeting from the summary title, SHALL happen only when that run's completion is recorded.

#### Scenario: Superseded run finishes late
- **WHEN** run A for a meeting is superseded by run B, and run A then produces a result
- **THEN** run A's result is discarded, the meeting's stored status stays as run B left it, and the meeting is not renamed from run A's title

#### Scenario: Superseded run fails late
- **WHEN** run A is superseded by run B and run A then fails
- **THEN** the meeting's stored status is not set to failed by run A

#### Scenario: Transcript save fails at start
- **WHEN** starting a run fails because the transcript data cannot be saved
- **THEN** the start request returns an error and the run is recorded as failed, not left pending
