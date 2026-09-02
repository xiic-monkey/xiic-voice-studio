import { useEffect, useRef, useState } from "react";
import { Pause, Play, Volume2, VolumeX } from "lucide-react";
import { formatAudioTime, sliderProgress } from "../utils";

/** 自绘音频播放器：进度 / 音量滑杆带 CSS 变量填充，src 变化即重置。 */
export function AudioPlayer({ src, label = "音频播放器" }: { src?: string; label?: string }) {
  const audioRef = useRef<HTMLAudioElement>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(1);
  const [muted, setMuted] = useState(false);

  useEffect(() => {
    const audio = audioRef.current;
    audio?.pause();
    if (audio) audio.currentTime = 0;
    setIsPlaying(false);
    setCurrentTime(0);
    setDuration(0);
  }, [src]);

  async function togglePlayback() {
    const audio = audioRef.current;
    if (!audio || !src) return;
    if (audio.paused) {
      try {
        await audio.play();
      } catch {
        setIsPlaying(false);
      }
    } else {
      audio.pause();
    }
  }

  function seek(value: number) {
    const audio = audioRef.current;
    if (!audio || !Number.isFinite(value)) return;
    audio.currentTime = value;
    setCurrentTime(value);
  }

  function changeVolume(value: number) {
    const audio = audioRef.current;
    if (!audio) return;
    audio.volume = value;
    audio.muted = value === 0;
    setVolume(value);
    setMuted(value === 0);
  }

  function toggleMuted() {
    const audio = audioRef.current;
    if (!audio) return;
    const nextMuted = !muted;
    audio.muted = nextMuted;
    setMuted(nextMuted);
  }

  const progress = duration > 0 ? (currentTime / duration) * 100 : 0;
  const audibleVolume = muted ? 0 : volume;

  return (
    <div className="custom-audio-player" aria-label={label}>
      <audio
        ref={audioRef}
        className="custom-audio-element"
        src={src}
        preload="metadata"
        onLoadedMetadata={(event) => setDuration(Number.isFinite(event.currentTarget.duration) ? event.currentTarget.duration : 0)}
        onDurationChange={(event) => setDuration(Number.isFinite(event.currentTarget.duration) ? event.currentTarget.duration : 0)}
        onTimeUpdate={(event) => setCurrentTime(event.currentTarget.currentTime)}
        onPlay={() => setIsPlaying(true)}
        onPause={() => setIsPlaying(false)}
        onEnded={() => setIsPlaying(false)}
      />
      <button
        className="audio-play"
        type="button"
        title={isPlaying ? "暂停" : "播放"}
        aria-label={isPlaying ? "暂停" : "播放"}
        onClick={togglePlayback}
        disabled={!src}
      >
        {isPlaying ? <Pause size={15} /> : <Play size={15} />}
      </button>
      <span className="audio-time">{formatAudioTime(currentTime)}</span>
      <input
        className="audio-slider audio-seek"
        type="range"
        min="0"
        max={duration || 0}
        step="0.01"
        value={Math.min(currentTime, duration || 0)}
        style={sliderProgress(progress)}
        aria-label="播放进度"
        aria-valuetext={`${formatAudioTime(currentTime)} / ${formatAudioTime(duration)}`}
        onChange={(event) => seek(Number(event.target.value))}
        disabled={!src || duration <= 0}
      />
      <span className="audio-time">{formatAudioTime(duration)}</span>
      <button
        className="audio-mute"
        type="button"
        title={muted ? "取消静音" : "静音"}
        aria-label={muted ? "取消静音" : "静音"}
        onClick={toggleMuted}
        disabled={!src}
      >
        {muted || volume === 0 ? <VolumeX size={15} /> : <Volume2 size={15} />}
      </button>
      <input
        className="audio-slider audio-volume"
        type="range"
        min="0"
        max="1"
        step="0.01"
        value={audibleVolume}
        style={sliderProgress(audibleVolume * 100)}
        aria-label="音量"
        aria-valuetext={`${Math.round(audibleVolume * 100)}%`}
        onChange={(event) => changeVolume(Number(event.target.value))}
        disabled={!src}
      />
    </div>
  );
}
