'use client';

import React from 'react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  buildBufferBars,
  buildChannelLine,
  buildLevelBar,
  buildModelIndicators,
  buildModelLines,
  DIAR_STATE_CLASS,
  MODEL_STATE_CLASS,
  type BufferBar,
} from '@/lib/diarization-status-lines';
import type {
  ChannelPipelineFill,
  DiarChannelStatus,
  RecordingTelemetry,
} from '@/services/diarizationStatusService';

/**
 * Two compact per-channel status lines plus one model-indicator row, rendered
 * to the right of the animated recording indicator
 * (online-diarization-telemetry).
 *
 * Each line reports only its own channel: its diarization counters and, as
 * proportional bars, the fills of the buffers that gate this channel's
 * operations (voice-activity dispatch, pending speech, recording mix). The row
 * beneath shows one colour-coded indicator per model kind, so model health is
 * visible without hovering; the tooltip carries the detail.
 */

const BufferBarView: React.FC<{ bar: BufferBar }> = ({ bar }) => (
  <span className="inline-flex items-center gap-0.5 align-middle" title={bar.detail}>
    <span className="inline-block w-6 h-1.5 rounded-sm bg-gray-200 overflow-hidden">
      <span
        className={`block h-full ${bar.fired ? 'bg-gray-800' : 'bg-gray-500'}`}
        style={{ width: `${bar.percent}%` }}
      />
    </span>
    <span className="text-[8px] text-gray-400 leading-none">{bar.label}</span>
  </span>
);

const ChannelRow: React.FC<{
  prefix: string;
  line: DiarChannelStatus;
  fill: ChannelPipelineFill;
}> = ({ prefix, line, fill }) => (
  <div className={`flex items-center gap-1 ${DIAR_STATE_CLASS[line.state]}`}>
    <span className="font-semibold">{prefix}</span>
    <span className="inline-flex items-center gap-1">
      <BufferBarView bar={buildLevelBar(fill.level)} />
      {buildBufferBars(fill).map((bar) => (
        <BufferBarView key={bar.label} bar={bar} />
      ))}
    </span>
    <span>{buildChannelLine(line, fill)}</span>
  </div>
);

interface DiarizationStatusLinesProps {
  telemetry: RecordingTelemetry;
}

export const DiarizationStatusLines: React.FC<DiarizationStatusLinesProps> = ({
  telemetry,
}) => {
  const { diarization, pipeline, models } = telemetry;
  const modelLines = buildModelLines(
    models.vad,
    models.asr,
    models.alignment,
    models.diarization,
    pipeline
  );
  const indicators = buildModelIndicators(
    models.vad,
    models.asr,
    models.alignment,
    models.diarization,
    [diarization.mic.state, diarization.sys.state]
  );

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <div
          data-testid="diarization-status-lines"
          className="ml-3 pl-3 border-l border-gray-200 font-mono text-[10px] leading-tight whitespace-nowrap text-left"
        >
          <ChannelRow prefix="MIC" line={diarization.mic} fill={pipeline.mic} />
          <ChannelRow prefix="SYS" line={diarization.sys} fill={pipeline.sys} />
          <div
            data-testid="model-indicators"
            className="flex items-center gap-2 mt-0.5"
          >
            {indicators.map((indicator) => (
              <span
                key={indicator.key}
                className="inline-flex items-center gap-1"
                title={indicator.title}
              >
                <span
                  data-state={indicator.state}
                  className={`inline-block w-1.5 h-1.5 rounded-full ${MODEL_STATE_CLASS[indicator.state]}`}
                />
                <span className="text-[8px] text-gray-500 leading-none">
                  {indicator.label}
                </span>
              </span>
            ))}
          </div>
        </div>
      </TooltipTrigger>
      <TooltipContent side="top" className="max-w-md">
        {modelLines.map((line) => (
          <p key={line} className="text-xs">
            {line}
          </p>
        ))}
      </TooltipContent>
    </Tooltip>
  );
};
