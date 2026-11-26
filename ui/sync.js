// Sync through your own Supabase project, end-to-end encrypted.
// The sync password never leaves this computer: a key and a row id are made
// from it (PBKDF2), the data is encrypted here (AES-GCM) and only the
// encrypted text is stored in Supabase.
const RavenSync = (() => {
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  function toBase64(bytes) {
    let text = "";
    for (let i = 0; i < bytes.length; i += 0x8000) {
      text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    }
    return btoa(text);
  }
  function fromBase64(text) {
    return Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
  }

  /// The encryption key and the row id, from the password. The same password
  /// gives the same key on every computer.
  async function derive(password) {
    const base = await crypto.subtle.importKey("raw", encoder.encode(password), "PBKDF2", false, ["deriveBits"]);
    const bits = new Uint8Array(
      await crypto.subtle.deriveBits({ name: "PBKDF2", salt: encoder.encode("raven-sync-v1"), iterations: 310000, hash: "SHA-256" }, base, 512),
    );
    const id = [...bits.slice(32)].map((b) => b.toString(16).padStart(2, "0")).join("");
    return { secret: toBase64(bits.slice(0, 32)), id };
  }

  async function key(secret) {
    return crypto.subtle.importKey("raw", fromBase64(secret), "AES-GCM", false, ["encrypt", "decrypt"]);
  }

  async function encrypt(secret, text) {
    const iv = crypto.getRandomValues(new Uint8Array(12));
    const data = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv }, await key(secret), encoder.encode(text)));
    return `${toBase64(iv)}.${toBase64(data)}`;
  }

  async function decrypt(secret, stored) {
    const [iv, data] = stored.split(".");
    const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: fromBase64(iv) }, await key(secret), fromBase64(data));
    return decoder.decode(plain);
  }

  function headers(config) {
    return {
      apikey: config.syncKey,
      Authorization: `Bearer ${config.syncKey}`,
      "Content-Type": "application/json",
    };
  }

  function base(config) {
    return config.syncUrl.replace(/\/+$/, "");
  }

  /// The encrypted data stored for this password, or null.
  async function pull(config) {
    const response = await fetch(`${base(config)}/rest/v1/raven_sync?id=eq.${config.syncId}&select=data,updated_at`, { headers: headers(config) });
    if (!response.ok) throw new Error(`Supabase: ${response.status} ${await response.text()}`);
    const rows = await response.json();
    return rows[0] || null;
  }

  async function push(config, data) {
    const response = await fetch(`${base(config)}/rest/v1/raven_sync`, {
      method: "POST",
      headers: { ...headers(config), Prefer: "resolution=merge-duplicates,return=minimal" },
      body: JSON.stringify({ id: config.syncId, data, updated_at: new Date().toISOString() }),
    });
    if (!response.ok) throw new Error(`Supabase: ${response.status} ${await response.text()}`);
  }

  const SETUP_SQL = `create table if not exists raven_sync (
  id text primary key,
  data text not null,
  updated_at timestamptz default now()
);
alter table raven_sync enable row level security;
create policy "raven sync" on raven_sync for all using (true) with check (true);`;

  return { derive, encrypt, decrypt, pull, push, SETUP_SQL };
})();
