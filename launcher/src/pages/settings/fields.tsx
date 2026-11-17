// The game's settings (settings.ini, described by src-tauri/resources/settings-schema.json): one row per field, the
// mouse block and the key bindings. The simple mode shows the common fields; the advanced mode adds the rare ones and
// the raw names and values.
import { useEffect, useRef, useState } from "react";
import { pick, useI18n, type Key } from "../../i18n";
import type { Schema, SchemaField } from "../../api";
import { useApp } from "../../ctx";
import { keyNameFromEvent, mouseNameFromEvent, wheelNameFromEvent } from "../../keys";

export type Values = Record<string, string>;
export type SetValue = (k: string, v: string | undefined) => void;

/** A slider drawn as a stepped scale: every tick shows its value, the current one is highlighted. */
function ticksOf(min: number, max: number, step: number): number[] {
  const span = max - min;
  if (span / step <= 12) return Array.from({ length: Math.round(span / step) + 1 }, (_, i) => Number((min + i * step).toFixed(4)));
  // Round tick values (1, 2, 5 times a power of ten), the ends included.
  const raw = span / 10;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const nice = [1, 2, 5, 10].map((m) => m * mag).find((v) => v >= raw) ?? raw;
  const out = [Number(min.toFixed(4))];
  for (let v = Math.ceil(min / nice) * nice; v <= max + 1e-9; v += nice) if (v > min + 1e-9) out.push(Number(v.toFixed(4)));
  return out;
}

function Scale({ f, current, label, onChange }: { f: SchemaField; current: number; label: string; onChange: (v: string | undefined) => void }) {
  const { t } = useI18n();
  const min = f.min ?? 0, max = f.max ?? 1, step = f.step ?? 0.01;
  const ticks = ticksOf(min, max, step);
  const nearest = ticks.reduce((a, b) => (Math.abs(b - current) < Math.abs(a - current) ? b : a), ticks[0]);
  const exact = Math.abs(nearest - current) < Math.max(step / 2, 1e-9);
  return (
    <div className="steps-scale" role="radiogroup" aria-label={`${label} (${t("settings.slider.value")})`}>
      {ticks.map((v) => (
        <button key={v} type="button" role="radio" aria-checked={exact && v === nearest} onClick={() => onChange(String(v))}>{v}</button>
      ))}
    </div>
  );
}

const WORDS: Record<string, Key> = { auto: "opt.auto", off: "opt.off", on: "opt.on", fps: "opt.fps", full: "opt.full", fit: "opt.fit", stretch: "opt.stretch" };

/** Words for the option values that have them (`auto`, `off`, `0`/`1` of a three-way switch...); others as they are. */
function optionText(t: ReturnType<typeof useI18n>["t"], f: SchemaField, o: string): string {
  if (WORDS[o]) return t(WORDS[o]);
  if (f.options.length <= 3 && f.options.includes("auto") && (o === "0" || o === "1")) return o === "1" ? t("opt.on") : t("opt.off");
  return o;
}

// Key names as the runtime writes them (`key:space`, `mouse_l`) or as the schema's defaults say them ("Left Shift").
const KEY_ALIASES: Record<string, string> = {
  "mouse left": "mouse_l", "mouse right": "mouse_r", "mouse middle": "mouse_m", "left shift": "lshift", "right shift": "rshift", "left ctrl": "lctrl",
  "right ctrl": "rctrl", "left alt": "lalt", "left arrow": "left", "up arrow": "up", "right arrow": "right", "down arrow": "down", "caps lock": "capslock",
};
const NAMED_KEYS = ["space", "enter", "backspace", "tab", "lshift", "rshift", "lctrl", "rctrl", "lalt", "capslock", "mouse_l", "mouse_r", "mouse_m", "mouse_x1", "mouse_x2", "wheel_up", "wheel_down", "up", "down", "left", "right"];

/** One key in the player's language: "Espace", "Clic gauche", "Q". */
export function keyLabel(token: string, t: ReturnType<typeof useI18n>["t"]): string {
  const raw = token.trim().replace(/^key:/i, "");
  const low = raw.toLowerCase();
  const id = KEY_ALIASES[low] ?? low;
  if (NAMED_KEYS.includes(id)) return t(`key.${id}` as Key);
  return raw.length === 1 ? raw.toUpperCase() : raw.charAt(0).toUpperCase() + raw.slice(1);
}

export function FieldRow({ f, value, error, onChange }: { f: SchemaField; value: string | undefined; error?: string; onChange: (v: string | undefined) => void }) {
  const { t, lang } = useI18n();
  const { advanced } = useApp();
  const id = `f-${f.key}`;
  const shown = value ?? f.default ?? "";
  let control;
  switch (f.type) {
    case "bool":
      control = (
        <div className="duo" role="radiogroup" aria-label={pick(f.label, lang)} id={id}>
          <button type="button" role="radio" aria-checked={shown !== "1"} onClick={() => onChange("0")}>{t("common.no")}</button>
          <button type="button" role="radio" aria-checked={shown === "1"} onClick={() => onChange("1")}>{t("common.yes")}</button>
        </div>
      );
      break;
    case "enum":
      control = (
        <select id={id} value={value ?? ""} onChange={(e) => onChange(e.target.value || undefined)}>
          <option value="">{t("common.default")} ({optionText(t, f, f.default ?? "")})</option>
          {f.options.map((o) => <option key={o} value={o}>{optionText(t, f, o)}</option>)}
        </select>
      );
      break;
    default: {
      const list = f.presets.length ? f.presets : f.options;
      const num = Number(shown);
      const slide = f.slider && f.min !== null && f.max !== null && f.step !== null;
      control = (
        <>
          {slide && <Scale f={f} current={Number.isFinite(num) ? num : Number(f.default)} label={pick(f.label, lang)} onChange={onChange} />}
          <input id={id} className="mono" value={value ?? ""} placeholder={f.default ?? ""} list={list.length ? `${id}-list` : undefined}
            inputMode={f.type === "size" || f.type === "choice_int" ? "text" : "decimal"} onChange={(e) => onChange(e.target.value === "" ? undefined : e.target.value)} aria-invalid={!!error} />
          {list.length > 0 && <datalist id={`${id}-list`}>{list.map((o) => <option key={o} value={o} />)}</datalist>}
        </>
      );
    }
  }
  return (
    <div className={`setting ${error ? "setting-bad" : ""} ${f.slider ? "setting-wide" : ""}`}>
      <div className="setting-text">
        <label htmlFor={id}>{pick(f.label, lang)}</label>
        {pick(f.help, lang) && <div className="hint">{pick(f.help, lang)}</div>}
        {advanced && <div className="mono tiny muted">{f.key}</div>}
        {error && <div className="bad-text">{t("settings.invalid")} : {error}</div>}
      </div>
      <div className="setting-control">
        {value !== undefined && <button type="button" className="link small reset" onClick={() => onChange(undefined)}>{t("common.reset")}</button>}
        {control}
      </div>
    </div>
  );
}

// Mouse-as-gyro, as the engine reads it (runtime/input.c, mouse_gyro_cfg; runtime/cfg.c ranges).
const MG_GAIN = 2.3933; // the game's camera turns this many times the virtual controller's yaw at its default motion sensitivity
const MG = { sens: [0.002, 1], axis: [0.1, 4], cm: [0.1, 200], dpi: [100, 32000], dpiDefault: 800, senseFloor: 0.00005 } as const;
export const MOUSE_KEYS = ["mouse_gyro_sens", "mouse_gyro_cm360", "mouse_gyro_dpi", "mouse_gyro_x", "mouse_gyro_y", "mouse_gyro_invert_y"];
const clampTo = (v: number, [lo, hi]: readonly [number, number]) => Math.min(hi, Math.max(lo, v));
const cmFromSens = (sens: number, dpi: number) => 360 / (sens * MG_GAIN) / dpi * 2.54;
const sensFromCm = (cm: number, dpi: number) => 360 / (cm / 2.54 * dpi) / MG_GAIN;
const round = (v: number, d: number) => Number(v.toFixed(d));
const numOr = (s: string | undefined, d: number) => { const n = Number((s ?? "").replace(",", ".")); return s !== undefined && s.trim() !== "" && Number.isFinite(n) ? n : d; };

function SliderRow({ id, label, hint, error, value, def, range, step, log, onChange }: {
  id: string; label: string; hint: string; error?: string; value: string | undefined; def: number; range: readonly [number, number]; step: number; log?: boolean; onChange: (v: string | undefined) => void;
}) {
  const { advanced } = useApp();
  const [lo, hi] = range;
  const cur = clampTo(numOr(value, def), range);
  const pos = log ? Math.round(1000 * Math.log(cur / lo) / Math.log(hi / lo)) : cur;
  const dec = Math.max(0, Math.ceil(-Math.log10(step)));
  return (
    <div className={`mouse-row ${error ? "setting-bad" : ""}`}>
      <div className="setting-text">
        <label htmlFor={id}>{label}</label>
        <div className="hint">{hint}</div>
        {advanced && <div className="mono tiny muted">{id.replace(/^m-/, "")}</div>}
        {error && <div className="bad-text">{error}</div>}
      </div>
      <input type="range" aria-label={label} min={log ? 0 : lo} max={log ? 1000 : hi} step={log ? 1 : step} value={pos}
        onChange={(e) => { const p = Number(e.target.value); onChange(String(round(log ? lo * (hi / lo) ** (p / 1000) : p, dec))); }} />
      <input id={id} className="mono mouse-num" value={value ?? ""} placeholder={String(def)} inputMode="decimal" aria-invalid={!!error}
        onChange={(e) => onChange(e.target.value === "" ? undefined : e.target.value)} />
    </div>
  );
}

export function MouseAim({ values, errors, setValue }: { values: Values; errors: Record<string, string>; setValue: SetValue }) {
  const { t } = useI18n();
  const { advanced } = useApp();
  const cmMode = numOr(values.mouse_gyro_cm360, 0) > 0;
  const dpi = clampTo(numOr(values.mouse_gyro_dpi, MG.dpiDefault), MG.dpi);
  const sens = clampTo(numOr(values.mouse_gyro_sens, 0.04), MG.sens);
  const cm = clampTo(numOr(values.mouse_gyro_cm360, 0), MG.cm);
  const rawSens = sensFromCm(cm, dpi);
  const used = clampTo(rawSens, [MG.senseFloor, 1]);
  const toCm = () => setValue("mouse_gyro_cm360", String(round(clampTo(cmFromSens(sens, dpi), MG.cm), 1)));
  const toSens = () => {
    setValue("mouse_gyro_sens", String(round(clampTo(used, MG.sens), 3)));
    setValue("mouse_gyro_cm360", undefined);
  };
  const dpiRow = <SliderRow id="m-mouse_gyro_dpi" label={t("mouse.dpi")} hint={t("mouse.dpi.hint")} error={errors.mouse_gyro_dpi} value={values.mouse_gyro_dpi} def={MG.dpiDefault} range={MG.dpi} step={50} log onChange={(v) => setValue("mouse_gyro_dpi", v)} />;
  return (
    <div className="set-block">
      <div className="set-block-head">
        <h3>{t("mouse.title")}</h3>
        <div className="duo" role="radiogroup" aria-label={t("mouse.mode")}>
          <button type="button" role="radio" aria-checked={!cmMode} onClick={() => cmMode && toSens()}>{t("mouse.mode.sens")}</button>
          <button type="button" role="radio" aria-checked={cmMode} onClick={() => !cmMode && toCm()}>{t("mouse.mode.cm")}</button>
        </div>
      </div>
      {cmMode ? (
        <>
          <SliderRow id="m-mouse_gyro_cm360" label={t("mouse.cm")} hint={t("mouse.cm.hint")} error={errors.mouse_gyro_cm360} value={values.mouse_gyro_cm360} def={30} range={MG.cm} step={0.1} log onChange={(v) => setValue("mouse_gyro_cm360", v)} />
          {dpiRow}
          <p className="hint" aria-live="polite">{t("mouse.eq.cm", { cm: round(cm, 1), dpi, sens: round(used, 4) })}{used !== rawSens && ` ${t("mouse.eq.clamped", { sens: round(used, 5) })}`}</p>
        </>
      ) : (
        <>
          <SliderRow id="m-mouse_gyro_sens" label={t("mouse.sens")} hint={t("mouse.sens.hint")} error={errors.mouse_gyro_sens} value={values.mouse_gyro_sens} def={0.04} range={MG.sens} step={0.001} log onChange={(v) => setValue("mouse_gyro_sens", v)} />
          {dpiRow}
          <p className="hint" aria-live="polite">{t("mouse.eq.sens", { cm: round(cmFromSens(sens, dpi), 1), dpi })}</p>
        </>
      )}
      <SliderRow id="m-mouse_gyro_x" label={t("mouse.h")} hint={t("mouse.h.hint")} error={errors.mouse_gyro_x} value={values.mouse_gyro_x} def={1} range={MG.axis} step={0.1} onChange={(v) => setValue("mouse_gyro_x", v)} />
      <SliderRow id="m-mouse_gyro_y" label={t("mouse.v")} hint={t("mouse.v.hint")} error={errors.mouse_gyro_y} value={values.mouse_gyro_y} def={1} range={MG.axis} step={0.1} onChange={(v) => setValue("mouse_gyro_y", v)} />
      <label className="check">
        <input type="checkbox" checked={values.mouse_gyro_invert_y === "1"} onChange={(e) => setValue("mouse_gyro_invert_y", e.target.checked ? "1" : undefined)} />
        {" "}{t("mouse.invert")} <span className="muted small">({t("mouse.invert.hint")})</span>
      </label>
      {advanced && <p className="hint">{t("mouse.eq.ref")}</p>}
      <div className="row between set-block-foot">
        <p className="hint">{t("mouse.help")}</p>
        <button type="button" className="btn btn-ghost btn-small" onClick={() => MOUSE_KEYS.forEach((k) => setValue(k, undefined))}>{t("mouse.defaults")}</button>
      </div>
    </div>
  );
}

/** A binding value as words: "key:space, mouse_l" -> "Espace", "Clic gauche". */
function bindingWords(v: string, t: ReturnType<typeof useI18n>["t"]): string[] {
  return v.split(",").map((s) => s.trim()).filter((s) => s && s.toLowerCase() !== "none").map((s) => keyLabel(s, t));
}

export function Bindings({ schema, values, errors, setValue }: { schema: Schema; values: Values; errors: Record<string, string>; setValue: SetValue }) {
  const { t, lang } = useI18n();
  const { advanced } = useApp();
  const [capturing, setCapturing] = useState<string | null>(null);
  const [captureMsg, setCaptureMsg] = useState<string | null>(null);
  const stop = useRef<(() => void) | null>(null);

  useEffect(() => () => stop.current?.(), []);

  const startCapture = (id: string) => {
    stop.current?.();
    const key = `bind_${id.toLowerCase()}`;
    setCapturing(id);
    setCaptureMsg(null);
    const finish = () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("mousedown", onMouse, true);
      window.removeEventListener("wheel", onWheel, true);
      stop.current = null;
      setCapturing(null);
    };
    const add = (name: string | null) => {
      if (!name || !schema.bindings.key_names.includes(name)) {
        setCaptureMsg(name ? `${name}: ${t("settings.invalid")}` : t("settings.invalid"));
        return;
      }
      const cur = values[key];
      const parts = cur ? cur.split(",").map((s) => s.trim()).filter((s) => s && s.toLowerCase() !== "none") : [];
      if (!parts.some((p) => p.toLowerCase() === `key:${name}`)) parts.push(`key:${name}`);
      setValue(key, parts.join(", "));
    };
    function onKey(e: KeyboardEvent) {
      e.preventDefault();
      e.stopPropagation();
      if (e.code === "Escape") return finish();
      add(keyNameFromEvent(e));
      finish();
    }
    function onMouse(e: MouseEvent) {
      e.preventDefault();
      e.stopPropagation();
      add(mouseNameFromEvent(e));
      finish();
    }
    function onWheel(e: WheelEvent) {
      e.preventDefault();
      e.stopPropagation();
      const name = wheelNameFromEvent(e);
      if (!name) return;   // a sideways scroll: keep waiting
      add(name);
      finish();
    }
    // Listen from the next tick so the click that started the capture is not taken as the new binding.
    window.setTimeout(() => {
      window.addEventListener("keydown", onKey, true);
      window.addEventListener("mousedown", onMouse, true);
      window.addEventListener("wheel", onWheel, { capture: true, passive: false });
    }, 0);
    stop.current = finish;
  };

  const rows = [
    ...schema.bindings.buttons.map((b) => ({ ...b, keyOnly: false })),
    ...schema.bindings.key_only_buttons.map((b) => ({ ...b, keyOnly: true })),
  ];

  return (
    <div className="set-block">
      <div className="set-block-head"><h3>{t("settings.keys.s3")}</h3></div>
      <p className="hint">{advanced ? t("settings.controls.help") : t("settings.keys.simple")}</p>
      {advanced && <p className="hint">{t("settings.keys.note")}</p>}
      {captureMsg && <p className="bad-text">{captureMsg}</p>}
      <table className="bindings">
        <thead>
          <tr><th scope="col">{t("settings.keys.action")}</th><th scope="col">{t("settings.keys.keys")}</th><th /></tr>
        </thead>
        <tbody>
          {rows.map((r) => {
            const key = `bind_${r.id.toLowerCase()}`;
            const err = errors[key];
            const v = values[key];
            return (
              <tr key={r.id} className={err ? "setting-bad" : ""}>
                <th scope="row">
                  {pick(r.label, lang)}
                  {advanced && <div className="mono tiny muted">{key}{r.keyOnly ? ` · ${t("settings.keyonly")}` : ""}</div>}
                </th>
                <td>
                  {advanced ? (
                    <input className="mono" value={v ?? ""} placeholder={`${t("common.default")} : ${r.default}`} aria-label={key}
                      onChange={(e) => setValue(key, e.target.value === "" ? undefined : e.target.value)} aria-invalid={!!err} />
                  ) : (
                    <span className="keys">
                      {v === undefined ? r.default.split(",").map((k) => <kbd key={k} className="kbd-default">{keyLabel(k, t)}</kbd>) : bindingWords(v, t).map((k) => <kbd key={k}>{k}</kbd>)}
                    </span>
                  )}
                  {err && <div className="bad-text">{err}</div>}
                </td>
                <td className="nowrap bind-actions">
                  <button type="button" className={`btn btn-small ${capturing === r.id ? "btn-alt" : "btn-ghost"}`} onClick={() => (capturing === r.id ? stop.current?.() : startCapture(r.id))}>
                    {capturing === r.id ? t("settings.capture.wait") : t("settings.capture")}
                  </button>
                  <button type="button" className={`btn btn-ghost btn-small reset ${v === undefined ? "reset-off" : ""}`} disabled={v === undefined} onClick={() => setValue(key, undefined)}>{t("common.reset")}</button>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
