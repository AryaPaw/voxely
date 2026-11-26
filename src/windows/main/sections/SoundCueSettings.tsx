import { Play } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { SettingsGroup } from "../../../components/settings/SettingsGroup";
import { Button } from "../../../components/ui/button";
import { Card, CardContent } from "../../../components/ui/card";
import { Label } from "../../../components/ui/label";
import { RadioGroup, RadioGroupItem } from "../../../components/ui/radio-group";
import { Slider } from "../../../components/ui/slider";
import { api, type CuePreset } from "../../../lib/api";
import {
  CUE_KINDS,
  CUE_PRESETS,
  cueScenario,
  decodeCueBank,
  type CueBank,
} from "../../../lib/cue-preview";
import { type Messages } from "../../../lib/i18n";

type PreviewPlayer = {
  context: AudioContext;
  volume: GainNode;
  clip: { source: AudioBufferSourceNode; fade: GainNode } | null;
};

function fadeOut(player: PreviewPlayer) {
  const clip = player.clip;
  if (!clip) return;
  player.clip = null;
  const now = player.context.currentTime;
  clip.fade.gain.setTargetAtTime(0, now, 0.002);
  clip.source.stop(now + 0.01);
}

function useCuePlayback() {
  const owner = useRef<PreviewPlayer>(null);
  const request = useRef(0);
  const [volume, setVolume] = useState(65);
  const [playing, setPlaying] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(
    () => () => {
      request.current++;
      const player = owner.current;
      owner.current = null;
      if (player) {
        player.clip = null;
        void player.context.close().catch(() => {});
      }
    },
    [],
  );

  async function play(id: string, samples: Float32Array, sampleRate: number) {
    const ticket = ++request.current;
    if (owner.current) fadeOut(owner.current);
    setPlaying(null);
    setFailed(false);
    try {
      if (!owner.current) {
        const context = new AudioContext();
        const volumeNode = context.createGain();
        volumeNode.gain.value = volume / 100;
        volumeNode.connect(context.destination);
        owner.current = { context, volume: volumeNode, clip: null };
      }
      const player = owner.current;
      const { context } = player;
      await context.resume();
      if (request.current !== ticket) return;
      const buffer = context.createBuffer(1, samples.length, sampleRate);
      buffer.getChannelData(0).set(samples);
      const source = context.createBufferSource();
      const fade = context.createGain();
      source.buffer = buffer;
      source.connect(fade);
      fade.connect(player.volume);
      source.onended = () => {
        source.disconnect();
        fade.disconnect();
        if (player.clip?.source !== source) return;
        player.clip = null;
        setPlaying(null);
      };
      source.start();
      player.clip = { source, fade };
      setPlaying(id);
    } catch {
      if (request.current === ticket) setFailed(true);
    }
  }

  function changeVolume(value: number) {
    setVolume(value);
    const player = owner.current;
    if (player) player.volume.gain.setTargetAtTime(value / 100, player.context.currentTime, 0.015);
  }

  return { volume, changeVolume, playing, failed, play };
}

export function SoundCueSettings({
  selected,
  enabled = true,
  copy,
  onChange,
}: {
  selected: CuePreset;
  enabled?: boolean;
  copy: Messages;
  onChange: (preset: CuePreset) => void;
}) {
  const id = useId();
  const [bank, setBank] = useState<CueBank | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [retry, setRetry] = useState(0);
  const { volume, changeVolume, playing, failed, play } = useCuePlayback();

  useEffect(() => {
    let cancelled = false;
    void api
      .cuePreviews()
      .then(decodeCueBank)
      .then((next) => {
        if (!cancelled) setBank(next);
      })
      .catch(() => {
        if (!cancelled) setLoadError(true);
      });
    return () => {
      cancelled = true;
    };
  }, [retry]);

  const names = {
    classic: copy.cueClassic,
    soft: copy.cueSoft,
    warm: copy.cueWarm,
  };
  const labels = { start: copy.cueStart, stop: copy.cueStop };
  return (
    <SettingsGroup title={copy.cueTitle}>
      <div className="space-y-3 pt-2">
        {!enabled ? <p className="text-xs text-muted-foreground">{copy.cueDisabledHint}</p> : null}
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
          <span className="text-muted-foreground">{copy.cuePreviewVolume}</span>
          <Slider
            className="min-h-6 w-36 sm:w-48 [&_[data-slot=slider-thumb]]:size-4"
            value={[volume]}
            aria-label={copy.cuePreviewVolume}
            onValueChange={([value]) => changeVolume(value)}
          />
          <span className="w-9 text-right text-muted-foreground tabular-nums">{volume}%</span>
        </div>

        {loadError ? (
          <div className="flex flex-wrap items-center gap-2">
            <p role="alert" className="text-sm text-destructive">
              {copy.cueLoadError}
            </p>
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => {
                setLoadError(false);
                setRetry(retry + 1);
              }}
            >
              {copy.cueReload}
            </Button>
          </div>
        ) : null}
        {!bank && !loadError ? (
          <p role="status" className="text-xs text-muted-foreground">
            {copy.cueLoading}
          </p>
        ) : null}
        {failed ? (
          <p role="alert" className="text-sm text-destructive">
            {copy.cuePlaybackError}
          </p>
        ) : null}

        <RadioGroup
          aria-label={copy.cueTitle}
          value={selected}
          onValueChange={(value) => {
            const preset = CUE_PRESETS.find((option) => option === value);
            if (preset) onChange(preset);
          }}
        >
          {CUE_PRESETS.map((preset) => {
            const clips = bank?.presets.get(preset);
            const itemId = `${id}-${preset}`;
            const sequenceId = `${preset}-sequence`;
            return (
              <Card
                key={preset}
                size="sm"
                className={selected === preset ? "ring-ring/65" : undefined}
              >
                <CardContent className="grid gap-2.5">
                  <div className="flex items-center gap-2">
                    <RadioGroupItem id={itemId} value={preset} />
                    <Label htmlFor={itemId} className="cursor-pointer">
                      {names[preset]}
                    </Label>
                  </div>
                  <div className="grid grid-cols-2 gap-2 min-[700px]:grid-cols-3">
                    {CUE_KINDS.map((kind) => {
                      const clip = clips?.[kind];
                      const clipId = `${preset}-${kind}`;
                      return (
                        <Button
                          key={kind}
                          type="button"
                          variant={playing === clipId ? "secondary" : "outline"}
                          className="flex h-auto min-h-11 flex-col items-stretch gap-0.5 px-2 py-1.5 text-left text-xs"
                          aria-label={`${names[preset]} – ${labels[kind]}`}
                          disabled={!clip}
                          onClick={() => {
                            if (bank && clip) void play(clipId, clip.samples, bank.sampleRate);
                          }}
                        >
                          <span>{labels[kind]}</span>
                          <span className="flex h-3 items-center gap-1" aria-hidden="true">
                            <span
                              className="flex h-full min-w-0 flex-1 items-center gap-px text-primary-text"
                              data-waveform=""
                            >
                              {clip?.peaks.map((peak, index) => (
                                <span
                                  key={index}
                                  className="min-w-0 flex-1 rounded-full bg-current"
                                  style={{ height: `${Math.max(12, peak)}%` }}
                                />
                              ))}
                            </span>
                            {clip ? (
                              <span className="text-[10px] text-muted-foreground tabular-nums">
                                {clip.durationMs} {copy.cueMillis}
                              </span>
                            ) : null}
                          </span>
                        </Button>
                      );
                    })}
                    <Button
                      type="button"
                      variant={playing === sequenceId ? "secondary" : "outline"}
                      className="col-span-2 h-auto min-h-11 text-xs min-[700px]:col-span-1"
                      aria-label={`${names[preset]} – ${copy.cueScenarioStop}`}
                      disabled={!clips}
                      onClick={() => {
                        if (bank && clips)
                          void play(
                            sequenceId,
                            cueScenario(clips.start.samples, clips.stop.samples, bank.sampleRate),
                            bank.sampleRate,
                          );
                      }}
                    >
                      <Play aria-hidden="true" />
                      {copy.cueScenarioStop}
                    </Button>
                  </div>
                </CardContent>
              </Card>
            );
          })}
        </RadioGroup>
      </div>
    </SettingsGroup>
  );
}
