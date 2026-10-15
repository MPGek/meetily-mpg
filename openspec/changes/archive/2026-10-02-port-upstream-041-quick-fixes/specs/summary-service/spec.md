# Spec Delta

## MODIFIED Requirements

### Requirement: Transcript chunking for large inputs
The system SHALL split transcripts into chunks that fit within the model's token threshold when the full text exceeds available context. Every character of the transcript SHALL appear in at least one chunk, including when a chunk is shortened to end at a sentence or word boundary, and chunking SHALL always make forward progress so it terminates for any chunk size and overlap.

#### Scenario: Chunk 50K-word transcript for Ollama
- **WHEN** transcript text exceeds the available token budget (context_size - 300)
- **THEN** system splits into sequential chunks and processes each, combining results

#### Scenario: Boundary snap-back longer than the overlap loses no text
- **WHEN** a chunk's last sentence boundary (". ") lies further back from the chunk's size limit than the configured overlap
- **THEN** the text between that boundary and the size limit SHALL appear at the start of the next chunk rather than being skipped

#### Scenario: Boundary inside the overlap is not used
- **WHEN** the only sentence or word boundary in a chunk lies within the overlap length from the chunk's start
- **THEN** the system SHALL end the chunk at its size limit instead of at that boundary, so the next chunk still starts after the current one

#### Scenario: Overlap equal to the chunk size still terminates
- **WHEN** the configured overlap is equal to or larger than the chunk size
- **THEN** each new chunk SHALL start at least one character after the previous chunk's start, and chunking SHALL finish with the final chunk ending at the end of the text

#### Scenario: Multi-byte text is split on character boundaries
- **WHEN** the transcript contains multi-byte (non-ASCII) characters near a chunk boundary
- **THEN** every chunk SHALL be valid text that starts and ends on a character boundary

## ADDED Requirements

### Requirement: Claude summaries use the response's text block
When generating a summary with the Anthropic Claude provider, the system SHALL use the first text block of the response as the summary. Non-text blocks, such as reasoning ("thinking") blocks that models with built-in reasoning return before the answer, SHALL be ignored. The output token budget for a Claude summary request SHALL leave room for reasoning tokens as well as the answer.

#### Scenario: Response begins with a thinking block
- **WHEN** Claude returns a response whose content is a thinking block followed by a text block
- **THEN** the system SHALL return the text block's content as the summary and SHALL NOT fail to parse the response

#### Scenario: Plain text response
- **WHEN** Claude returns a response containing a single text block
- **THEN** the system SHALL return that block's text as the summary

#### Scenario: Response contains no text block
- **WHEN** Claude returns a response whose content has no text block
- **THEN** the system SHALL surface an error stating that the response contained no text content, rather than returning an empty summary
