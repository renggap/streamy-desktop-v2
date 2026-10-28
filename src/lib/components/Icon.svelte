<script lang="ts">
  // Minimal stroke icon set (Lucide-style paths), so no icon dependency is needed.
  type Name =
    | "mic" | "mic-off" | "radio" | "square" | "settings" | "server" | "plus" | "pencil"
    | "trash" | "folder" | "refresh" | "x" | "check" | "alert" | "info" | "record"
    | "chevron-down" | "arrow-left" | "music" | "send" | "play" | "pause" | "skip-back"
    | "skip-forward" | "repeat" | "list-end" | "volume" | "headphones" | "arrow-up" | "arrow-down"
    | "file-plus" | "layout" | "upload" | "download";
  let { name, size = 16 }: { name: Name; size?: number } = $props();
  const FILLED = new Set<Name>(["record", "play", "pause", "skip-back", "skip-forward"]);

  const paths: Record<Name, string> = {
    mic: "M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z M19 10v2a7 7 0 0 1-14 0v-2 M12 19v3",
    "mic-off": "M2 2l20 20 M18.89 13.23A7 7 0 0 0 19 12v-2 M5 10v2a7 7 0 0 0 12 5 M15 9.34V5a3 3 0 0 0-5.68-1.33 M9 9v3a3 3 0 0 0 5.12 2.12 M12 19v3",
    radio: "M4.9 19.1a10 10 0 0 1 0-14.2 M7.8 16.2a6 6 0 0 1 0-8.4 M16.2 7.8a6 6 0 0 1 0 8.4 M19.1 4.9a10 10 0 0 1 0 14.2 M12 12m-2 0a2 2 0 1 0 4 0a2 2 0 1 0-4 0",
    square: "M6 6h12v12H6z",
    settings: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z",
    server: "M3 4h18v6H3z M3 14h18v6H3z M7 7h.01 M7 17h.01",
    plus: "M12 5v14 M5 12h14",
    pencil: "M17 3a2.85 2.85 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z",
    trash: "M3 6h18 M8 6V4h8v2 M19 6l-1 14H6L5 6",
    folder: "M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2Z",
    refresh: "M21 12a9 9 0 1 1-2.64-6.36L21 8 M21 3v5h-5",
    x: "M18 6 6 18 M6 6l12 12",
    check: "M20 6 9 17l-5-5",
    alert: "M12 9v4 M12 17h.01 M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0Z",
    info: "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z M12 16v-4 M12 8h.01",
    record: "M12 12m-6 0a6 6 0 1 0 12 0a6 6 0 1 0-12 0",
    "chevron-down": "m6 9 6 6 6-6",
    "arrow-left": "M19 12H5 M12 19l-7-7 7-7",
    music: "M9 18V5l12-2v13 M6 18m-3 0a3 3 0 1 0 6 0a3 3 0 1 0-6 0 M18 16m-3 0a3 3 0 1 0 6 0a3 3 0 1 0-6 0",
    send: "m22 2-7 20-4-9-9-4Z M22 2 11 13",
    play: "M6 4l14 8-14 8Z",
    pause: "M7 4h3v16H7z M14 4h3v16h-3z",
    "skip-back": "M19 20 9 12l10-8z M5 19V5",
    "skip-forward": "m5 4 10 8-10 8z M19 5v14",
    repeat: "m17 2 4 4-4 4 M3 11v-1a4 4 0 0 1 4-4h14 M7 22l-4-4 4-4 M21 13v1a4 4 0 0 1-4 4H3",
    "list-end": "M16 12H3 M16 6H3 M10 18H3 M21 6v10a2 2 0 0 1-2 2h-5 M16 16l-2 2 2 2",
    volume: "M11 5 6 9H2v6h4l5 4z M15.54 8.46a5 5 0 0 1 0 7.07 M19.07 4.93a10 10 0 0 1 0 14.14",
    headphones: "M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3",
    "arrow-up": "m5 12 7-7 7 7 M12 19V5",
    "arrow-down": "M12 5v14 M19 12l-7 7-7-7",
    "file-plus": "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z M14 2v4a2 2 0 0 0 2 2h4 M9 15h6 M12 18v-6",
    layout: "M3 3h7v9H3z M14 3h7v5h-7z M14 12h7v9h-7z M3 16h7v5H3z",
    download: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4 M7 10l5 5 5-5 M12 15V3",
    upload: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4 M17 8l-5-5-5 5 M12 3v12",
  };
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={FILLED.has(name) ? "currentColor" : "none"}
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={paths[name]} />
</svg>
