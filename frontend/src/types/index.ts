export interface Message {
  id: string;
  content: string;
  timestamp: string;
}

export interface Transcript {
  id: string;
  text: string;
  timestamp: string; // Wall-clock time (e.g., "14:30:05")
  sequence_id?: number;
  chunk_start_time?: number; // Legacy field
  is_partial?: boolean;
  confidence?: number;
  // NEW: Recording-relative timestamps for playback sync
  audio_start_time?: number; // Seconds from recording start (e.g., 125.3)
  audio_end_time?: number;   // Seconds from recording start (e.g., 128.6)
  duration?: number;          // Segment duration in seconds (e.g., 3.3)
  source_device?: string;    // "Microphone" or "System"
  speaker?: string;          // Speaker ID from diarization
  speaker_label?: string;    // User-assigned speaker name
  speaker_matched_by?: string; // 'user' | 'auto' | 'fallback'
  speaker_match_score?: number; // cosine similarity 0..1 for auto-matched
  tokens?: TranscriptToken[]; // Word-level timestamps for token diarization
}

// One word token with recording-relative start/end times (seconds).
export interface TranscriptToken {
  text: string;
  start: number;
  end: number;
  refined?: boolean; // true when timestamps came from CTC forced alignment
}

export interface TranscriptUpdate {
  text: string;
  timestamp: string; // Wall-clock time for reference
  source: string;
  sequence_id: number;
  chunk_start_time: number; // Legacy field
  is_partial: boolean;
  confidence: number;
  // NEW: Recording-relative timestamps for playback sync
  audio_start_time: number; // Seconds from recording start
  audio_end_time: number;   // Seconds from recording start
  duration: number;          // Segment duration in seconds
  source_device: string;    // "Microphone" or "System"
  speaker?: string;         // Speaker ID from diarization
  tokens?: TranscriptToken[]; // Word-level timestamps from the transcription engine
}

export interface Block {
  id: string;
  type: string;
  content: string;
  color: string;
}

export interface Section {
  title: string;
  blocks: Block[];
}

export interface Summary {
  [key: string]: Section;
}

export interface ApiResponse {
  message: string;
  num_chunks: number;
  data: any[];
}

export interface SummaryResponse {
  status: string;
  summary: Summary;
  raw_summary?: string;
  usage?: {
    prompt_tokens: number;
    completion_tokens: number;
    total_tokens: number;
  };
}

// BlockNote-specific types
export type SummaryFormat = 'legacy' | 'markdown' | 'blocknote';

export interface BlockNoteBlock {
  id: string;
  type: string;
  props?: Record<string, any>;
  content?: any[];
  children?: BlockNoteBlock[];
}

export interface SummaryDataResponse {
  markdown?: string;
  summary_json?: BlockNoteBlock[];
  // Legacy format fields
  MeetingName?: string;
  _section_order?: string[];
  [key: string]: any; // For legacy section data
}

// Pagination types for optimized transcript loading
export interface MeetingMetadata {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
  folder_path?: string;
}

export interface PaginatedTranscriptsResponse {
  transcripts: Transcript[];
  total_count: number;
  has_more: boolean;
}

// Transcript segment data for virtualized display
export interface TranscriptSegmentData {
  id: string;
  timestamp: number; // audio_start_time in seconds
  endTime?: number; // audio_end_time in seconds
  hasAudioTime?: boolean; // true when the transcript has a real audio_start_time
  text: string;
  confidence?: number;
  source_device?: string; // "Microphone" or "System"
  speaker?: string;       // Speaker ID from diarization
  speaker_label?: string; // User-assigned speaker name
  speaker_matched_by?: string; // 'user' | 'auto' | 'fallback'
  speaker_match_score?: number; // cosine similarity 0..1
  // Live word-level diarization: display sub-rows for a split block
  // (live-word-level-diarization). Present only while recording.
  blocks?: LiveTranscriptBlock[];
}

// One display sub-row emitted by live word-level diarization for a transcript
// block that spans more than one live speaker.
export interface LiveTranscriptBlock {
  start: number; // recording-relative seconds
  end: number;
  text: string;
  speaker: string; // raw cluster label (drives color/identity)
  display_name?: string; // recognized/bound name to show instead of the label
  matched_by?: string; // 'user' | 'auto'
  match_score?: number;
}

// Payload of the `live-transcript-blocks` event: the current display revision
// of one parent transcript block. Children carry no sequence_id of their own.
export interface LiveTranscriptBlocks {
  parent_sequence_id: number;
  source_device: string;
  revision: number;
  blocks: LiveTranscriptBlock[];
}

// Speaker diarization types
export interface DiarizationProgress {
  meeting_id: string;
  status: string; // "loading" | "decoding" | "diarizing" | "matching" | "complete" | "failed"
  progress: number; // 0-100
  message: string;
}

export interface DiarizationResult {
  meeting_id: string;
  segments_labeled: number;
  speakers_found: number;
}

export type SpeakerMap = Record<string, string>; // speaker_id -> label
