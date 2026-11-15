// One view of the launcher's own update for the whole window: the backend's events keep it fresh. Kept apart from the
// components so that the shell can follow it without loading their code.
import { useEffect, useState } from "react";
import { onUpdateState, updateApi, type UpdateView } from "../api";

let latest: UpdateView | null = null;
const subscribers = new Set<(v: UpdateView) => void>();
let listening = false;

export function setUpdateView(v: UpdateView) {
  latest = v;
  for (const s of subscribers) s(v);
}

function listenOnce() {
  if (listening) return;
  listening = true;
  void onUpdateState(setUpdateView).catch(() => {});
  if (!latest) void updateApi.status().then(setUpdateView).catch(() => {});
}

export function useLauncherUpdate(): UpdateView | null {
  const [v, setV] = useState<UpdateView | null>(latest);
  useEffect(() => {
    listenOnce();
    subscribers.add(setV);
    if (latest) setV(latest);
    return () => { subscribers.delete(setV); };
  }, []);
  return v;
}
