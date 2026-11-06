// The Rust bridge: proto types (serde's externally tagged JSON), commands, events, blobs.
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Timestamp = number; // ms since the Unix epoch
export type Service = "IMessage" | "Sms" | "Rcs";
export type BlobKind = "Attachment" | "PhotoThumbnail" | "Photo" | "VoiceMemo";

export interface Attachment {
  guid: string;
  mime_type: string | null;
  file_name: string | null;
  total_bytes: number;
}
export interface Message {
  guid: string;
  chat_guid: string;
  sender: string | null;
  is_from_me: boolean;
  text: string | null;
  date: Timestamp;
  attachments: Attachment[];
}
export interface Chat {
  guid: string;
  display_name: string | null;
  participants: string[];
  service: Service;
  last_message: Message | null;
  my_address: string | null;
}
export interface Calendar {
  id: string;
  title: string;
  color: number;
  writable: boolean;
}
export interface CalendarEvent {
  id: string;
  calendar_id: string;
  title: string;
  location: string | null;
  notes: string | null;
  start: Timestamp;
  end: Timestamp;
  all_day: boolean;
}
export type NewEvent = Omit<CalendarEvent, "id">;
export interface NoteFolder {
  id: string;
  name: string;
  account: string;
  count: number;
}
export interface NoteSummary {
  id: string;
  folder_id: string;
  title: string;
  snippet: string;
  modified: Timestamp;
}
export interface Note {
  summary: NoteSummary;
  body: string;
}
export interface VoiceMemo {
  id: string;
  title: string;
  created: Timestamp;
  duration_secs: number;
  bytes: number;
}
export interface Album {
  id: string;
  title: string;
  count: number;
}
export interface Asset {
  id: string;
  created: Timestamp;
  width: number;
  height: number;
  is_video: boolean;
  favorite: boolean;
}
export interface Location {
  lat: number;
  lon: number;
  accuracy_m: number;
  at: Timestamp;
}
export interface FindMyDevice {
  id: string;
  name: string;
  kind: "Apple" | "Beacon";
  location: Location | null;
  battery: number | null;
  model: string;
}

export type ServerEvent =
  | { NewMessage: Message }
  | "CalendarChanged"
  | "NotesChanged"
  | "VoiceMemosChanged"
  | "PhotosChanged";

export type Status =
  | { state: "NotPaired" }
  | { state: "Connecting" }
  | { state: "Connected"; detail: { server_name: string } }
  | { state: "Disconnected"; detail: string };

export interface ApiError {
  code: "Unauthorized" | "NotFound" | "PermissionDenied" | "Unsupported" | "BadRequest" | "Internal";
  message: string;
}

export interface Notifications {
  enabled: boolean;
  messages: boolean;
  imessage: boolean;
  sms: boolean;
  show_sender: boolean;
  show_preview: boolean;
  skip_codes: boolean;
  only_when_unfocused: boolean;
  muted_chats: string[];
  calendar_reminders: boolean;
  reminder_minutes: number;
  photos: boolean;
  notes: boolean;
  voice_memos: boolean;
  low_battery: boolean;
  low_battery_percent: number;
  beacon_located: boolean;
  connection: boolean;
  quiet_hours: boolean;
  quiet_start: string;
  quiet_end: string;
}

export interface ConfigView {
  server: { host: string; port: number; fingerprint: string } | null;
  autofill: {
    enabled: boolean;
    from_server: boolean;
    notify: boolean;
    clear_after: number;
    http: { port: number; token: string } | null;
    gmail: string[];
  };
  notifications: Notifications;
  beacon: { name: string; keys: number } | null;
}

export interface Bootstrap {
  status: Status;
  config: ConfigView;
  primary: boolean;
  startup_error: string | null;
  platform: "linux" | "macos";
  device_name: string;
  user: string;
  version: string;
  config_path: string;
  dev_tab: string | null;
  dev_open_first_chat: boolean;
  dev_force_width: number | null;
}

export interface AudioState {
  id: string | null;
  title: string;
  duration: number;
  position: number;
  paused: boolean;
  ended: boolean;
  error: string | null;
}

/** Error text for toasts, from an ApiError, a string or anything else. */
export function errorText(err: unknown): string {
  if (typeof err === "string") return err;
  if (err && typeof err === "object" && "message" in err) return String((err as { message: unknown }).message);
  return String(err);
}

type Variant<T, K extends string> = { [P in K]: T };

async function call<T, K extends string>(req: object | string, key: K): Promise<T> {
  const res = await invoke<Variant<T, K> | string>("request", { req });
  if (typeof res === "object" && res && key in res) return (res as Variant<T, K>)[key];
  throw { code: "Internal", message: `unexpected reply ${JSON.stringify(res).slice(0, 80)}` } satisfies ApiError;
}

async function ok(req: object): Promise<void> {
  await invoke("request", { req });
}

export const api = {
  chats: (limit = 300) => call<Chat[], "Chats">({ ListChats: { limit, before: null } }, "Chats"),
  messages: (chat_guid: string, limit: number, before: Timestamp | null) =>
    call<Message[], "Messages">({ ListMessages: { chat_guid, limit, before } }, "Messages"),
  sendText: (chat_guid: string, text: string) => ok({ SendText: { chat_guid, text } }),
  sendAttachment: (chat_guid: string, file: File) =>
    file.arrayBuffer().then((buf) =>
      invoke<void>("send_attachment", new Uint8Array(buf), {
        headers: { "x-chat": encodeURIComponent(chat_guid), "x-name": encodeURIComponent(file.name || "attachment") },
      }),
    ),
  calendars: () => call<Calendar[], "Calendars">("ListCalendars", "Calendars"),
  events: (from: Timestamp, to: Timestamp) => call<CalendarEvent[], "Events">({ ListEvents: { from, to } }, "Events"),
  createEvent: (event: NewEvent) => call<CalendarEvent, "Event">({ CreateEvent: event }, "Event"),
  deleteEvent: (id: string) => ok({ DeleteEvent: { id } }),
  noteFolders: () => call<NoteFolder[], "NoteFolders">("ListNoteFolders", "NoteFolders"),
  notes: (folder_id: string | null) => call<NoteSummary[], "Notes">({ ListNotes: { folder_id } }, "Notes"),
  note: (id: string) => call<Note, "Note">({ GetNote: { id } }, "Note"),
  createNote: (folder_id: string | null, body: string) =>
    call<{ id: string }, "Created">({ CreateNote: { folder_id, body } }, "Created"),
  updateNote: (id: string, body: string) => ok({ UpdateNote: { id, body } }),
  voiceMemos: () => call<VoiceMemo[], "VoiceMemos">("ListVoiceMemos", "VoiceMemos"),
  albums: () => call<Album[], "Albums">("ListAlbums", "Albums"),
  assets: (album_id: string | null, offset: number, limit: number) =>
    call<Asset[], "Assets">({ ListAssets: { album_id, offset, limit } }, "Assets"),
  findMy: () => call<FindMyDevice[], "FindMyDevices">("ListFindMyDevices", "FindMyDevices"),
};

export const native = {
  bootstrap: () => invoke<Bootstrap>("bootstrap"),
  pair: (uri: string, name: string) => invoke<ConfigView>("pair", { uri, name }),
  beaconSetup: (name: string) => invoke<ConfigView>("beacon_setup", { name }),
  saveNotifications: (notifications: Notifications) => invoke<ConfigView>("save_notifications", { notifications }),
  saveAutofill: (options: { enabled: boolean; from_server: boolean; notify: boolean; clear_after: number }) =>
    invoke<ConfigView>("save_autofill", { options }),
  autofillHttp: (enable: boolean) => invoke<ConfigView>("autofill_http", { enable }),
  gmailAdd: (address: string, appPassword: string) => invoke<ConfigView>("gmail_add", { address, appPassword }),
  gmailRemove: (address: string) => invoke<ConfigView>("gmail_remove", { address }),
  saveBlob: (kind: BlobKind, id: string, name: string) => invoke<string>("save_blob", { kind, id, name }),
  audioPlay: (id: string, title: string, duration: number) => invoke<void>("audio_play", { id, title, duration }),
  audioToggle: () => invoke<void>("audio_toggle"),
  audioSeek: (secs: number) => invoke<void>("audio_seek", { secs }),
  audioStop: () => invoke<void>("audio_stop"),
  audioState: () => invoke<AudioState>("audio_state"),
};

/** A URL the webview can load a server blob from (served by Rust's `bb://` handler). */
export function blobUrl(kind: BlobKind, id: string): string {
  return convertFileSrc(`blob/${kind}/${id}`, "bb");
}

export function tileUrl(): string {
  // Leaflet fills {z}/{x}/{y}; keep the template braces unencoded.
  return convertFileSrc("tile/", "bb") + "{z}%2F{x}%2F{y}";
}

export function onServerEvent(fn: (event: ServerEvent) => void): Promise<UnlistenFn> {
  return listen<ServerEvent>("server-event", (e) => fn(e.payload));
}

export function onStatus(fn: (status: Status) => void): Promise<UnlistenFn> {
  return listen<Status>("status", (e) => fn(e.payload));
}
