import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import L from "leaflet";
import { ArrowLeft, BatteryLow, BatteryMedium, Laptop, LocateFixed, MapPin, Minus, Plus, Radar, RotateCw, Smartphone, Tablet, Watch, Headphones } from "lucide-react";
import { api, errorText, tileUrl, type FindMyDevice } from "@/lib/bb";
import { keys } from "@/lib/events";
import { NARROW_BELOW, useApp, useWidth } from "@/lib/app";
import { ago } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Badge, Card, Center, EmptyState, IconButton, Spinner } from "@/components/ui";

const REFRESH_MS = 60_000;

function deviceIcon(d: FindMyDevice) {
  if (d.kind === "Beacon") return Radar;
  const m = d.model.toLowerCase();
  if (m.includes("mac")) return Laptop;
  if (m.includes("ipad")) return Tablet;
  if (m.includes("watch")) return Watch;
  if (m.includes("airpods") || m.includes("beats")) return Headphones;
  return Smartphone;
}

export default function FindMyPage() {
  const { connected, config } = useApp();
  const [ref, width] = useWidth<HTMLDivElement>();
  const [selected, setSelected] = useState<string | null>(null);
  const [showMap, setShowMap] = useState(false);
  const devices = useQuery({ queryKey: keys.findMy, queryFn: api.findMy, enabled: connected, refetchInterval: REFRESH_MS });
  const narrow = width > 0 && width < NARROW_BELOW;
  const list = devices.data ?? [];
  const lowAt = config.notifications.low_battery_percent / 100;

  const pick = (id: string) => {
    setSelected(id);
    setShowMap(true);
  };

  const listCard = (
    <Card className="flex min-w-0 flex-col overflow-hidden" style={narrow ? { flex: 1 } : { width: Math.min(340, Math.max(270, width * 0.3)) }}>
      <div className="flex items-center gap-2 p-4 pb-2">
        <div className="min-w-0 flex-1">
          <h1 className="text-[15px] font-semibold text-fg">Find My</h1>
          <div className="text-xs text-subtle">{devices.dataUpdatedAt ? `Updated ${ago(devices.dataUpdatedAt)}` : "Your devices and this PC"}</div>
        </div>
        {devices.isFetching ? <Spinner /> : <IconButton icon={RotateCw} label="Refresh" onClick={() => void devices.refetch()} />}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {devices.isLoading ? (
          <Center><Spinner /></Center>
        ) : devices.error ? (
          <EmptyState icon={MapPin} title="Couldn't load devices" hint={errorText(devices.error)} />
        ) : !list.length ? (
          <EmptyState icon={MapPin} title="No devices" hint={connected ? "Run bbd findmy-login on the Mac." : "Connect to your Mac in Settings."} />
        ) : (
          list.map((d) => {
            const Icon = deviceIcon(d);
            const low = d.battery !== null && d.battery < lowAt;
            return (
              <button
                key={d.id}
                onClick={() => pick(d.id)}
                className={cn("flex w-full items-center gap-3 rounded-control px-2.5 py-2.5 text-left transition-colors duration-150", d.id === selected ? "bg-border" : "hover:bg-raised")}
              >
                <span className="flex size-9 shrink-0 items-center justify-center rounded-full border border-border-strong bg-raised text-fg">
                  <Icon className="size-4" />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-2">
                    <span className="truncate text-[13px] font-medium text-fg">{d.name}</span>
                    {d.kind === "Beacon" && <Badge className="h-5 px-2 text-[10.5px]">beacon</Badge>}
                  </span>
                  <span className="block truncate text-xs text-subtle">
                    {d.location ? `${ago(d.location.at)} · ±${Math.round(d.location.accuracy_m)} m` : "No location yet"}
                  </span>
                </span>
                {d.battery !== null && (
                  <span className={cn("flex shrink-0 items-center gap-1 text-xs", low ? "text-danger" : "text-muted")}>
                    {low ? <BatteryLow className="size-4" /> : <BatteryMedium className="size-4" />}
                    {Math.round(d.battery * 100)}%
                  </span>
                )}
              </button>
            );
          })
        )}
      </div>
    </Card>
  );

  return (
    <div ref={ref} className="flex h-full gap-4">
      {(!narrow || !showMap) && listCard}
      {(!narrow || showMap) && (
        <Card className="relative min-w-0 flex-1 overflow-hidden">
          <DeviceMap devices={list} selected={selected} onSelect={setSelected} />
          {narrow && (
            <div className="absolute top-3 left-3 z-[500]">
              <IconButton icon={ArrowLeft} label="Back" className="border border-border bg-surface shadow-lift" onClick={() => setShowMap(false)} />
            </div>
          )}
        </Card>
      )}
    </div>
  );
}

function pinHtml(d: FindMyDevice, selected: boolean) {
  // Unselected: muted grey; selected: the inverted white one. A dark ring keeps both
  // readable on the monochrome tiles.
  const fill = selected ? "#fafafa" : "#a1a1a1";
  const stroke = "#0a0a0a";
  const label = selected ? `<div style="position:absolute;top:-30px;left:50%;transform:translateX(-50%);white-space:nowrap;background:#171717;border:1px solid #262626;color:#fafafa;border-radius:999px;padding:2px 10px;font:500 11px var(--font-sans);box-shadow:0 8px 32px rgb(0 0 0/.67)">${escapeHtml(d.name)}</div>` : "";
  return `<div style="position:relative;width:18px;height:18px">${label}<div style="width:18px;height:18px;border-radius:999px;background:${fill};border:3px solid ${stroke};box-shadow:0 0 0 1px ${selected ? "#fafafa" : "#404040"},0 8px 24px rgb(0 0 0/.8)"></div></div>`;
}

function escapeHtml(s: string) {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

function DeviceMap({ devices, selected, onSelect }: { devices: FindMyDevice[]; selected: string | null; onSelect: (id: string) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const map = useRef<L.Map | null>(null);
  const layer = useRef<L.LayerGroup | null>(null);
  const fitted = useRef(false);

  useEffect(() => {
    if (!host.current) return;
    const m = L.map(host.current, { zoomControl: false, attributionControl: true, worldCopyJump: true }).setView([30, 0], 2);
    m.attributionControl.setPrefix(false);
    L.tileLayer(tileUrl(), { maxZoom: 19, attribution: "© OpenStreetMap contributors" }).addTo(m);
    layer.current = L.layerGroup().addTo(m);
    map.current = m;
    const ro = new ResizeObserver(() => m.invalidateSize());
    ro.observe(host.current);
    return () => {
      ro.disconnect();
      m.remove();
      map.current = null;
    };
  }, []);

  // Pins: monochrome, the selected one inverted, accuracy as a faint circle.
  useEffect(() => {
    const m = map.current;
    const g = layer.current;
    if (!m || !g) return;
    g.clearLayers();
    const located = devices.filter((d) => d.location);
    for (const d of located) {
      const at: L.LatLngTuple = [d.location!.lat, d.location!.lon];
      const isSel = d.id === selected;
      L.circle(at, { radius: d.location!.accuracy_m, color: "#a1a1a1", weight: 1, opacity: 0.35, fillColor: "#a1a1a1", fillOpacity: 0.06, interactive: false }).addTo(g);
      L.marker(at, { icon: L.divIcon({ className: "bb-pin", html: pinHtml(d, isSel), iconSize: [18, 18], iconAnchor: [9, 9] }), zIndexOffset: isSel ? 1000 : 0, title: d.name })
        .on("click", () => onSelect(d.id))
        .addTo(g);
    }
    if (!fitted.current && located.length) {
      fitted.current = true;
      const bounds = L.latLngBounds(located.map((d) => [d.location!.lat, d.location!.lon] as L.LatLngTuple));
      m.fitBounds(bounds.pad(0.4), { maxZoom: 14, animate: false });
    }
  }, [devices, selected, onSelect]);

  useEffect(() => {
    const d = devices.find((x) => x.id === selected);
    if (d?.location && map.current) map.current.flyTo([d.location.lat, d.location.lon], Math.max(map.current.getZoom(), 15), { duration: 0.6 });
  }, [selected]); // eslint-disable-line react-hooks/exhaustive-deps

  const fitAll = () => {
    const located = devices.filter((d) => d.location);
    if (map.current && located.length) map.current.flyToBounds(L.latLngBounds(located.map((d) => [d.location!.lat, d.location!.lon] as L.LatLngTuple)).pad(0.4), { maxZoom: 14, duration: 0.6 });
  };

  return (
    <>
      <div ref={host} className="bb-map absolute inset-0" />
      <div className="absolute top-3 right-3 z-[500] flex flex-col gap-1 rounded-full border border-border bg-surface p-1 shadow-lift">
        <IconButton icon={Plus} label="Zoom in" size="iconSm" onClick={() => map.current?.zoomIn()} />
        <IconButton icon={Minus} label="Zoom out" size="iconSm" onClick={() => map.current?.zoomOut()} />
        <IconButton icon={LocateFixed} label="Show all devices" size="iconSm" onClick={fitAll} />
      </div>
    </>
  );
}
