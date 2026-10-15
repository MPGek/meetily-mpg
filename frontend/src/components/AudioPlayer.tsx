'use client';

import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from 'react';
import { Pause, Play } from 'lucide-react';
import { useAudioPlayer } from '@/hooks/useAudioPlayer';

export interface AudioPlayerHandle {
  playFrom: (startTime: number) => Promise<void>;
}

export interface PlaybackState {
  isPlaying: boolean;
  endedCount: number;
}

interface AudioPlayerProps {
  audioPath: string | null;
  onPlaybackStateChange?: (state: PlaybackState) => void;
  onTimeUpdate?: (time: number) => void;
}

function formatTime(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  if (hours > 0) {
    return `${hours}:${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
  }
  return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

export const AudioPlayer = forwardRef<AudioPlayerHandle, AudioPlayerProps>(
  function AudioPlayer({ audioPath, onPlaybackStateChange, onTimeUpdate }, ref) {
    const {
      isPlaying,
      currentTime,
      duration,
      error,
      endedCount,
      play,
      pause,
      seek,
      load,
      audioRef,
    } = useAudioPlayer(audioPath);
    const [dragging, setDragging] = useState(false);
    const [dragValue, setDragValue] = useState(0);
    const pendingPlayRef = useRef<number | null>(null);

    // Queue play-from when audio is still loading
    useEffect(() => {
      if (duration > 0 && pendingPlayRef.current !== null) {
        const t = pendingPlayRef.current;
        pendingPlayRef.current = null;
        void (async () => {
          await seek(t);
          await play();
        })();
      }
    }, [duration, seek, play]);

    useImperativeHandle(ref, () => ({
      playFrom: async (startTime: number) => {
        if (duration > 0) {
          await seek(startTime);
          await play();
        } else {
          pendingPlayRef.current = startTime;
          await load();
        }
      },
    }), [duration, seek, play, load]);

    // Notify parent of discrete playback state changes
    useEffect(() => {
      onPlaybackStateChange?.({ isPlaying, endedCount });
    }, [isPlaying, endedCount, onPlaybackStateChange]);

    // Report position (the element fires timeupdate ~4Hz)
    useEffect(() => {
      onTimeUpdate?.(currentTime);
    }, [currentTime, onTimeUpdate]);

    if (!audioPath) return null;

    const sliderValue = dragging ? dragValue : currentTime;
    const hasAudio = duration > 0 && !error;

    const commitDrag = () => {
      if (dragging) {
        setDragging(false);
        void seek(dragValue);
      }
    };

    return (
      <>
        <audio ref={audioRef} className="hidden" />
        <div className="border-b border-gray-200 bg-gray-50 px-3 py-2">
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => (isPlaying ? pause() : void play())}
              disabled={!hasAudio}
              aria-label={isPlaying ? 'Pause' : 'Play'}
              className="flex-shrink-0 w-8 h-8 flex items-center justify-center rounded-full bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
            >
              {isPlaying ? <Pause className="w-4 h-4" /> : <Play className="w-4 h-4" />}
            </button>
            <span className="text-xs text-gray-600 tabular-nums min-w-[38px]">{formatTime(sliderValue)}</span>
            <input
              type="range"
              min={0}
              max={duration || 0}
              step={0.05}
              value={Math.min(sliderValue, duration || 0)}
              disabled={!hasAudio}
              onChange={(e) => {
                setDragging(true);
                setDragValue(Number(e.target.value));
              }}
              onPointerUp={commitDrag}
              onKeyUp={commitDrag}
              onBlur={commitDrag}
              className="flex-1 h-1 accent-blue-600 cursor-pointer disabled:cursor-not-allowed"
            />
            <span className="text-xs text-gray-600 tabular-nums min-w-[38px]">{formatTime(duration)}</span>
          </div>
          {error && (
            <p className="text-xs text-red-600 mt-1">{error}</p>
          )}
        </div>
      </>
    );
  }
);
