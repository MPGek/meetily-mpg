'use client';

import React from 'react';
import {
  buildBufferTank,
  buildChannelLine,
  buildLevelBar,
  buildModelIndicators,
  DIAR_STATE_CLASS,
  MODEL_BLINK_CLASS,
  MODEL_STATE_CLASS,
  type ModelIndicator,
} from '@/lib/diarization-status-lines';
import type {
  ChannelPipelineFill,
  DiarChannelStatus,
  RecordingTelemetry,
} from '@/services/diarizationStatusService';

/**
 * Simplified live status block (online-diarization-telemetry): two compact
 * per-channel level lines plus one model-indicator row, rendered to the right
 * of the animated recording indicator.
 *
 * Each line shows only its channel's input level, its speech-recognition
 * buffer (a tank against the 25 s cap and a ring counting the silence to the
 * 500 ms dispatch trigger; live-asr-dispatch-latency) and state; the row beneath
 * shows one blinking indicator per model kind (green while the model is
 * processing, red while a request it has not started consuming is queued) and,
 * for STT/diarization, the number of blocks still waiting in their queues.
 */

/** Silence countdown to the dispatch trigger, as a 12 px ring. */
const SilenceRing: React.FC<{ fraction: number; stuck: boolean }> = ({ fraction, stuck }) => {
  const radius = 4.5;
  const circumference = 2 * Math.PI * radius;
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" className="flex-shrink-0" aria-hidden="true">
      <circle cx="6" cy="6" r={radius} fill="none" strokeWidth="2" className="stroke-gray-200" />
      <circle
        cx="6"
        cy="6"
        r={radius}
        fill="none"
        strokeWidth="2"
        className={stuck ? 'stroke-red-500' : 'stroke-amber-500'}
        strokeDasharray={`${circumference * (stuck ? 1 : fraction)} ${circumference}`}
        transform="rotate(-90 6 6)"
      />
    </svg>
  );
};

const ChannelRow: React.FC<{
  prefix: string;
  line: DiarChannelStatus;
  fill: ChannelPipelineFill;
  /** With diarization off the line carries no diarization state text. */
  diarizationOff: boolean;
}> = ({ prefix, line, fill, diarizationOff }) => {
  const level = buildLevelBar(fill.level);
  const tank = buildBufferTank(fill.pending);
  return (
    <div className={`flex items-center gap-1 ${DIAR_STATE_CLASS[line.state]}`}>
      <span className="font-semibold">{prefix}</span>
      <span
        className="inline-block w-12 h-1.5 rounded-sm bg-gray-200 overflow-hidden"
        title={level.detail}
      >
        <span
          className={`block h-full ${level.firing ? 'bg-red-500' : 'bg-gray-500'}`}
          style={{ width: `${level.percent}%` }}
        />
      </span>
      <span
        data-testid="asr-buffer-tank"
        className="relative inline-block w-16 h-1.5 rounded-sm bg-gray-200 overflow-hidden"
        title={tank.detail}
      >
        <span
          className={`absolute inset-y-0 left-0 ${tank.stuck ? 'bg-red-500' : 'bg-amber-500'}`}
          style={{ width: `${tank.pendingPercent}%` }}
        />
        <span
          className="absolute inset-y-0 bg-blue-500"
          style={{ left: `${tank.pendingPercent}%`, width: `${tank.openPercent}%` }}
        />
      </span>
      <span title={tank.detail}>
        <SilenceRing fraction={tank.ringFraction} stuck={tank.stuck} />
      </span>
      <span>
        {tank.stuck ? 'waiting for speech' : diarizationOff ? '' : buildChannelLine(line)}
      </span>
    </div>
  );
};

const IndicatorDot: React.FC<{ indicator: ModelIndicator }> = ({ indicator }) => (
  <span
    className="inline-flex items-center gap-1"
    title={`${indicator.title} - ${indicator.state} blink:${indicator.blink}`}
  >
    <span
      data-testid={`model-indicator-${indicator.key}`}
      data-state={indicator.state}
      data-blink={indicator.blink}
      className={`inline-block w-1.5 h-1.5 rounded-full ${
        indicator.blink === 'none'
          ? MODEL_STATE_CLASS[indicator.state]
          : MODEL_BLINK_CLASS[indicator.blink]
      }`}
    />
    <span className="text-[8px] text-gray-500 leading-none">
      {indicator.label}
      {indicator.pending ? ` ${indicator.pending}` : ''}
    </span>
  </span>
);

interface DiarizationStatusLinesProps {
  telemetry: RecordingTelemetry;
}

export const DiarizationStatusLines: React.FC<DiarizationStatusLinesProps> = ({
  telemetry,
}) => {
  const { diarization, pipeline, models } = telemetry;
  const indicators = buildModelIndicators(
    models.vad,
    models.asr,
    models.alignment,
    models.diarization,
    [diarization.mic.state, diarization.sys.state]
  );

  return (
    <div
      data-testid="diarization-status-lines"
      className="ml-3 pl-3 border-l border-gray-200 font-mono text-[10px] leading-tight whitespace-nowrap text-left"
    >
      <ChannelRow
        prefix="MIC"
        line={diarization.mic}
        fill={pipeline.mic}
        diarizationOff={diarization.mode === 'off'}
      />
      <ChannelRow
        prefix="SYS"
        line={diarization.sys}
        fill={pipeline.sys}
        diarizationOff={diarization.mode === 'off'}
      />
      <div data-testid="model-indicators" className="flex items-center gap-2 mt-0.5">
        {indicators.map((indicator) => (
          <IndicatorDot key={indicator.key} indicator={indicator} />
        ))}
      </div>
    </div>
  );
};
