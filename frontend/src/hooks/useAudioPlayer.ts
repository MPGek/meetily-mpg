import { useCallback, useEffect, useRef, useState } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';

export const useAudioPlayer = (audioPath: string | null) => {
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [error, setError] = useState<string | null>(null);
  // Incremented every time playback ends naturally
  const [endedCount, setEndedCount] = useState(0);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const retriedRef = useRef(false);
  const rangeEndRef = useRef<number | null>(null);

  // Attach element listeners and (re)load whenever the path changes
  useEffect(() => {
    const el = audioRef.current;
    if (!el || !audioPath) {
      return;
    }

    retriedRef.current = false;
    setCurrentTime(0);
    setDuration(0);
    setIsPlaying(false);
    setEndedCount(0);
    setError(null);

    el.src = convertFileSrc(audioPath);
    el.load();

    const onDurationChange = () => {
      setDuration(el.duration || 0);
      setError(null);
    };
    const onTimeUpdate = () => {
      setCurrentTime(el.currentTime);
      if (rangeEndRef.current !== null && el.currentTime >= rangeEndRef.current) {
        el.pause();
        rangeEndRef.current = null;
      }
    };
    const onPlay = () => {
      setIsPlaying(true);
      setError(null);
    };
    const onPause = () => setIsPlaying(false);
    const onEnded = () => {
      setIsPlaying(false);
      setCurrentTime(0);
      el.currentTime = 0;
      setEndedCount((c) => c + 1);
    };
    const onError = async () => {
      console.warn('Media element failed to load:', el.error);
      if (retriedRef.current) {
        setError('Failed to load audio file');
        return;
      }
      retriedRef.current = true;
      try {
        const wavPath = await invoke<string>('prepare_audio_for_playback', {
          filePath: audioPath
        });
        console.log('Transcoded WAV ready at:', wavPath);
        el.src = convertFileSrc(wavPath);
        el.load();
      } catch (e) {
        console.error('Transcode fallback failed:', e);
        setError('Failed to load audio file');
      }
    };

    el.addEventListener('durationchange', onDurationChange);
    el.addEventListener('timeupdate', onTimeUpdate);
    el.addEventListener('play', onPlay);
    el.addEventListener('pause', onPause);
    el.addEventListener('ended', onEnded);
    el.addEventListener('error', onError);

    return () => {
      el.removeEventListener('durationchange', onDurationChange);
      el.removeEventListener('timeupdate', onTimeUpdate);
      el.removeEventListener('play', onPlay);
      el.removeEventListener('pause', onPause);
      el.removeEventListener('ended', onEnded);
      el.removeEventListener('error', onError);
    };
  }, [audioPath]);

  // Release playback resources on unmount
  useEffect(() => {
    return () => {
      const el = audioRef.current;
      if (el) {
        el.pause();
        el.removeAttribute('src');
        el.load();
      }
    };
  }, []);

  const load = useCallback(async () => {
    const el = audioRef.current;
    if (!el || !audioPath) return;
    retriedRef.current = false;
    setError(null);
    el.src = convertFileSrc(audioPath);
    el.load();
  }, [audioPath]);

  const play = useCallback(async () => {
    const el = audioRef.current;
    if (!el) return;
    if (el.ended || (el.duration > 0 && el.currentTime >= el.duration - 0.05)) {
      el.currentTime = 0;
      setCurrentTime(0);
    }
    try {
      await el.play();
    } catch (e) {
      console.error('Error during playback:', e);
      setError('Failed to play audio');
    }
  }, []);

  const pause = useCallback(() => {
    audioRef.current?.pause();
  }, []);

  const seek = useCallback(async (time: number) => {
    const el = audioRef.current;
    if (!el) return;
    const clamped = Math.max(0, Math.min(time, el.duration || 0));
    el.currentTime = clamped;
    setCurrentTime(clamped);
    // Native behavior: a playing element keeps playing after a seek
  }, []);

  const playRange = useCallback(async (start: number, end: number) => {
    const el = audioRef.current;
    if (!el) return;
    rangeEndRef.current = end;
    const clampedStart = Math.max(0, Math.min(start, el.duration || 0));
    el.currentTime = clampedStart;
    setCurrentTime(clampedStart);
    try {
      await el.play();
    } catch (e) {
      console.error('Error during playRange:', e);
      setError('Failed to play audio');
      rangeEndRef.current = null;
    }
  }, []);

  return {
    isPlaying,
    currentTime,
    duration,
    error,
    endedCount,
    load,
    play,
    pause,
    seek,
    playRange,
    audioRef,
  };
};
