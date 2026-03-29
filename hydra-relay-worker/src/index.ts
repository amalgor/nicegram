import { connect } from "cloudflare:sockets";

interface Env {
  HYDRA_QUOTAS: KVNamespace;
  DEFAULT_DAILY_QUOTA: string;
}

// Telegram DC IP ranges for validation
const TELEGRAM_SUBNETS = [
  "149.154.",
  "91.108.",
  // Telegram Web WS endpoints
  "pluto.web.telegram.org",
  "venus.web.telegram.org",
  "aurora.web.telegram.org",
  "vesta.web.telegram.org",
  "flora.web.telegram.org",
];

function isAllowedTarget(target: string): boolean {
  const host = target.split(":")[0];
  return TELEGRAM_SUBNETS.some((prefix) => host.startsWith(prefix) || host.endsWith(prefix));
}

function todayKey(deviceId: string): string {
  const d = new Date();
  const date = `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, "0")}-${String(d.getUTCDate()).padStart(2, "0")}`;
  return `quota:${deviceId}:${date}`;
}

async function checkAndUpdateQuota(
  kv: KVNamespace,
  deviceId: string,
  bytes: number,
  defaultLimit: number,
): Promise<{ allowed: boolean; remaining: number }> {
  const key = todayKey(deviceId);
  const raw = await kv.get(key);
  let used = 0;
  let limit = defaultLimit;

  if (raw) {
    try {
      const data = JSON.parse(raw);
      used = data.bytes_used || 0;
      limit = data.bytes_limit || defaultLimit;
    } catch {
      // corrupted entry, reset
    }
  }

  const remaining = Math.max(0, limit - used);
  if (bytes > 0 && used + bytes <= limit) {
    await kv.put(
      key,
      JSON.stringify({ bytes_used: used + bytes, bytes_limit: limit }),
      { expirationTtl: 86400 * 2 },
    );
    return { allowed: true, remaining: remaining - bytes };
  }

  return { allowed: used < limit, remaining };
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    // Health check
    if (url.pathname === "/health") {
      return new Response("ok", { status: 200 });
    }

    // Quota check endpoint (GET)
    if (url.pathname === "/quota") {
      const deviceId = url.searchParams.get("device_id") || "anonymous";
      const defaultLimit = parseInt(env.DEFAULT_DAILY_QUOTA) || 52428800;
      const { remaining } = await checkAndUpdateQuota(env.HYDRA_QUOTAS, deviceId, 0, defaultLimit);
      return Response.json({ remaining, limit: defaultLimit });
    }

    // WebSocket upgrade for relay
    const upgradeHeader = request.headers.get("Upgrade");
    if (!upgradeHeader || upgradeHeader !== "websocket") {
      return new Response("Hydra Relay. Send WebSocket upgrade to connect.", {
        status: 426,
        headers: { "Content-Type": "text/plain" },
      });
    }

    const target = request.headers.get("X-Hydra-Target");
    if (!target) {
      return new Response("Missing X-Hydra-Target header (format: host:port)", { status: 400 });
    }

    if (!isAllowedTarget(target)) {
      return new Response("Target not in allowed list", { status: 403 });
    }

    const deviceId = request.headers.get("X-Hydra-Device") || "anonymous";
    const defaultLimit = parseInt(env.DEFAULT_DAILY_QUOTA) || 52428800;
    const { allowed, remaining } = await checkAndUpdateQuota(env.HYDRA_QUOTAS, deviceId, 0, defaultLimit);
    if (!allowed) {
      return new Response("Daily quota exceeded", { status: 429 });
    }

    // Parse target
    const parts = target.split(":");
    const hostname = parts[0];
    const port = parseInt(parts[1] || "443");

    const webSocketPair = new WebSocketPair();
    const [client, server] = Object.values(webSocketPair);
    server.accept();

    // Open TCP connection to target
    let tcpSocket: Socket;
    try {
      tcpSocket = connect(
        { hostname, port },
        { secureTransport: "off", allowHalfOpen: false },
      );
    } catch (e) {
      server.close(1011, `Failed to connect to ${target}: ${e}`);
      return new Response(null, { status: 101, webSocket: client });
    }

    let totalBytes = 0;

    // WS -> TCP: client sends data, we forward to TCP target
    server.addEventListener("message", (event: MessageEvent) => {
      const data = event.data;
      const writer = tcpSocket.writable.getWriter();
      if (data instanceof ArrayBuffer) {
        totalBytes += data.byteLength;
        writer.write(new Uint8Array(data));
      } else if (typeof data === "string") {
        const encoded = new TextEncoder().encode(data);
        totalBytes += encoded.byteLength;
        writer.write(encoded);
      }
      writer.releaseLock();
    });

    // TCP -> WS: target sends data, we forward to WS client
    (async () => {
      try {
        const reader = tcpSocket.readable.getReader();
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          if (value) {
            totalBytes += value.byteLength;
            server.send(value);
          }
        }
      } catch {
        // connection closed
      } finally {
        server.close(1000, "TCP connection closed");
        // Update quota with total bytes transferred
        if (totalBytes > 0) {
          await checkAndUpdateQuota(env.HYDRA_QUOTAS, deviceId, totalBytes, defaultLimit);
        }
      }
    })();

    // WS close -> TCP close
    server.addEventListener("close", () => {
      try {
        tcpSocket.close();
      } catch {
        // already closed
      }
    });

    return new Response(null, { status: 101, webSocket: client });
  },
};
