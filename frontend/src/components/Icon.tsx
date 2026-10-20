/**
 * Icone a linee dell'interfaccia, disegnate per Greatnote su una griglia di
 * 20×20 (tratto `currentColor`): niente font di icone né asset di terzi.
 */

const PATHS = {
  home: ["M3.5 9 10 3.5 16.5 9v7a1 1 0 0 1-1 1h-3v-5h-5v5h-3a1 1 0 0 1-1-1Z"],
  star: ["M10 2.8l2.2 4.6 5 .6-3.7 3.4 1 5L10 13.9l-4.5 2.5 1-5L2.8 8l5-.6Z"],
  notes: ["M5.5 2.5h6l3 3v12h-9Z", "M11.5 2.5v3h3", "M7.8 9.5h4.4M7.8 12.5h4.4"],
  "note-plus": ["M11 17.5H5.5v-15h6l3 3v4.5", "M11.5 2.5v3h3", "M15 12.5v5M12.5 15h5"],
  check: ["M10 17.5a7.5 7.5 0 1 0 0-15 7.5 7.5 0 0 0 0 15Z", "M6.8 10.2 9 12.4l4.2-4.6"],
  bell: ["M5 14V9a5 5 0 0 1 10 0v5l1.5 1.5h-13Z", "M8.3 17.5a1.8 1.8 0 0 0 3.4 0"],
  "bell-plus": ["M4.5 14V9a5 5 0 0 1 7.5-4.3", "M15.5 10.5V14l1.5 1.5H3", "M8.3 17.5a1.8 1.8 0 0 0 3.4 0", "M15.5 2.5v5M13 5h5"],
  file: [
    "M15.5 9.5 9.7 15.3a3.5 3.5 0 0 1-5-5L11 4a2.3 2.3 0 0 1 3.3 3.3l-6.2 6.2a1.1 1.1 0 0 1-1.6-1.6l5.6-5.6",
  ],
  calendar: ["M3.5 5h13v12h-13Z", "M3.5 8.5h13M7 3v3.5M13 3v3.5"],
  "calendar-plus": ["M3.5 5h13v12h-13Z", "M3.5 8.5h13M7 3v3.5M13 3v3.5", "M10 10.5v4.5M7.8 12.8h4.4"],
  template: ["M3.5 3.5h13v13h-13Z", "M3.5 7.5h13M8 7.5v9"],
  sparkle: [
    "M9 2.5l1.6 4.3 4.4 1.7-4.4 1.7L9 14.5l-1.6-4.3L3 8.5l4.4-1.7Z",
    "M15.5 12.5l.7 1.8 1.8.7-1.8.7-.7 1.8-.7-1.8-1.8-.7 1.8-.7Z",
  ],
  more: ["M4.6 10h.01M10 10h.01M15.4 10h.01"],
  trash: ["M4.5 6.5h11l-.8 10h-9.4l-.8-10Z", "M3.5 4.5h13M8 4.5V2.8h4v1.7M8 8.5v5.5M12 8.5v5.5"],
  notebook: ["M5.5 2.5h9a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-9Z", "M8 2.5v15", "M3.8 6h3M3.8 10h3M3.8 14h3"],
  tag: ["M3 3h6.5l7.5 7.5-6.5 6.5L3 9.5Z", "M6.5 6.5h.01"],
  "tag-plus": ["M3 3h6.5l3 3", "M3 3v6.5l7.5 7.5 2-2", "M6.5 6.5h.01", "M15.5 10v6M12.5 13h6"],
  space: ["M10 2.5l7 3.5v8l-7 3.5-7-3.5v-8Z", "M3 6l7 3.5L17 6M10 9.5v8"],
  search: ["M9 15a6 6 0 1 0 0-12 6 6 0 0 0 0 12Z", "M13.5 13.5l4 4"],
  plus: ["M10 4v12M4 10h12"],
  clock: ["M10 17.5a7.5 7.5 0 1 0 0-15 7.5 7.5 0 0 0 0 15Z", "M10 6v4l2.6 2"],
  archive: ["M3 4h14v3.5H3Z", "M4.5 7.5v9h11v-9", "M8 10.5h4"],
  inbox: ["M3 11l2-7.5h10L17 11v5.5H3Z", "M3 11h4l1 2h4l1-2h4"],
  collapse: ["M9.5 5l-5 5 5 5M15.5 5l-5 5 5 5"],
  expand: ["M12 3.5h4.5V8M8 16.5H3.5V12M16.5 3.5l-5 5M3.5 16.5l5-5"],
  grid: ["M3.5 3.5H9V9H3.5ZM11 3.5h5.5V9H11ZM3.5 11H9v5.5H3.5ZM11 11h5.5v5.5H11Z"],
  list: ["M7 5h9.5M7 10h9.5M7 15h9.5M3.5 5h.01M3.5 10h.01M3.5 15h.01"],
  export: ["M10 12.5V3M6.5 6.5 10 3l3.5 3.5", "M4 11v5.5h12V11"],
  link: [
    "M8.5 11.5a3.2 3.2 0 0 0 4.6 0l2.6-2.6a3.2 3.2 0 0 0-4.6-4.6l-.9.9",
    "M11.5 8.5a3.2 3.2 0 0 0-4.6 0l-2.6 2.6a3.2 3.2 0 0 0 4.6 4.6l.9-.9",
  ],
  x: ["M5.5 5.5l9 9M14.5 5.5l-9 9"],
  "chevron-right": ["M8 5l5 5-5 5"],
  "chevron-down": ["M5 8l5 5 5-5"],
  sidebar: ["M3 4h14v12H3Z", "M8 4v12"],
  restore: ["M4 10a6 6 0 1 0 1.8-4.3L4 7.5", "M4 3.5v4h4"],
} as const;

export type IconName = keyof typeof PATHS;

export default function Icon({
  name,
  size = 18,
  className,
}: {
  name: IconName;
  size?: number;
  className?: string;
}) {
  const dots = name === "more" || name === "tag" || name === "tag-plus" || name === "list";
  return (
    <svg
      aria-hidden="true"
      className={`icon${className ? ` ${className}` : ""}`}
      fill="none"
      focusable="false"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={dots && name === "more" ? 2.6 : 1.6}
      viewBox="0 0 20 20"
      width={size}
    >
      {PATHS[name].map((path) => (
        <path d={path} key={path} />
      ))}
    </svg>
  );
}
