import { useEffect, useState } from "react";
import { accountKey, api, type AccountRef } from "../api";

const cache = new Map<string, string | null>();

/** Голова скина аккаунта. Без скина или без сети — нейтральная заглушка того же размера. */
export function Avatar({ account, size = 32 }: { account?: AccountRef; size?: number }) {
  const key = account ? accountKey(account) : "";
  const [src, setSrc] = useState<string | null>(key ? cache.get(key) ?? null : null);
  useEffect(() => {
    if (!key || cache.has(key)) return;
    api.accountAvatar(key).then((url) => { cache.set(key, url); setSrc(url); }).catch(() => cache.set(key, null));
  }, [key]);
  return src
    ? <img className="head" src={src} width={size} height={size} alt="" style={{ width: size, height: size }} />
    : <div className="head" aria-hidden style={{ width: size, height: size }} />;
}
