import { useEffect, useRef } from "react";
import { api } from "../../lib/api";
import {
  drawOverlayWave,
  meterPollAllowed,
  overlayBarHeights,
  OVERLAY_BAR_COUNT,
} from "../../lib/overlay-wave";

export function OverlayWave({ active }: { active: boolean }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const levelsRef = useRef<number[]>([]);
  const barsRef = useRef<number[]>([]);
  const lastFrameRef = useRef<number | null>(null);
  const reducedMotion = useRef(
    typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  );

  useEffect(() => {
    if (!active) {
      levelsRef.current = [];
      barsRef.current = [];
      return;
    }
    let inFlight = false;
    const timer = window.setInterval(() => {
      if (!meterPollAllowed(inFlight)) {
        return;
      }
      inFlight = true;
      void api
        .meter()
        .then((sample) => {
          levelsRef.current = sample.levels ?? [];
        })
        .finally(() => {
          inFlight = false;
        });
    }, 50);
    return () => window.clearInterval(timer);
  }, [active]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      return;
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      return;
    }
    let frame = 0;
    lastFrameRef.current = null;
    const draw = (now: number) => {
      const previous = lastFrameRef.current;
      lastFrameRef.current = now;
      const dt = previous == null ? 1 / 60 : Math.min(0.05, (now - previous) / 1000);
      const dpr = window.devicePixelRatio || 1;
      const width = canvas.clientWidth;
      const height = canvas.clientHeight;
      if (canvas.width !== Math.floor(width * dpr) || canvas.height !== Math.floor(height * dpr)) {
        canvas.width = Math.floor(width * dpr);
        canvas.height = Math.floor(height * dpr);
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      const next = overlayBarHeights(
        levelsRef.current,
        active,
        barsRef.current,
        OVERLAY_BAR_COUNT,
        dt,
        reducedMotion.current,
      );
      barsRef.current = next;
      drawOverlayWave(ctx, width, height, active ? next : []);
      frame = window.requestAnimationFrame(draw);
    };
    frame = window.requestAnimationFrame(draw);
    return () => window.cancelAnimationFrame(frame);
  }, [active]);

  return <canvas ref={canvasRef} className="overlay-wave" aria-hidden="true" />;
}
