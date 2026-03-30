import { connect } from "cloudflare:sockets";

interface Env {
  HYDRA_QUOTAS: KVNamespace;
  DEFAULT_DAILY_QUOTA: string;
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
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname === "/health") {
      return new Response("ok", { status: 200 });
    }

    if (url.pathname === "/quota") {
      const deviceId = url.searchParams.get("device_id") || "anonymous";
      const defaultLimit = parseInt(env.DEFAULT_DAILY_QUOTA) || 52428800;
      const { remaining } = await checkAndUpdateQuota(env.HYDRA_QUOTAS, deviceId, 0, defaultLimit);
      return Response.json({ remaining, limit: defaultLimit });
    }

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

    const deviceId = request.headers.get("X-Hydra-Device") || "anonymous";
    const defaultLimit = parseInt(env.DEFAULT_DAILY_QUOTA) || 52428800;
    const { allowed } = await checkAndUpdateQuota(env.HYDRA_QUOTAS, deviceId, 0, defaultLimit);
    if (!allowed) {
      return new Response("Daily quota exceeded", { status: 429 });
    }

    console.log(`[relay] new connection: device=${deviceId}, target=${target}`);

    const parts = target.split(":");
    const hostname = parts[0];
    const port = parseInt(parts[1] || "443");

    const webSocketPair = new WebSocketPair();
    const [client, server] = Object.values(webSocketPair);
    server.accept();

    let tcpSocket: Socket;
    try {
      tcpSocket = connect(
        { hostname, port },
        { secureTransport: "off", allowHalfOpen: false },
      );
    } catch (e) {
      server.close(1011, `TCP connect failed: ${e}`);
      return new Response(null, { status: 101, webSocket: client });
    }

    let totalBytes = 0;
    const writer = tcpSocket.writable.getWriter();

    // WS -> TCP: set up listener BEFORE returning response
    server.addEventListener("message", (event: MessageEvent) => {
      const data = event.data;
      let bytes: Uint8Array;
      if (data instanceof ArrayBuffer) {
        bytes = new Uint8Array(data);
      } else if (typeof data === "string") {
        bytes = new TextEncoder().encode(data);
      } else {
        return;
      }
      totalBytes += bytes.byteLength;
      writer.write(bytes).catch(() => {
        try { server.close(1011, "TCP write failed"); } catch { /* noop */ }
      });
    });

    server.addEventListener("close", () => {
      writer.close().catch(() => {});
    });

    server.addEventListener("error", () => {
      writer.close().catch(() => {});
    });

    // TCP -> WS: pipe the readable through a WritableStream that sends to WS.
    // Use pipeTo which is natively supported and doesn't get cancelled.
    const tcpToWsTask = tcpSocket.readable.pipeTo(
      new WritableStream({
        write(chunk: Uint8Array) {
          totalBytes += chunk.byteLength;
          server.send(chunk);
        },
        close() {
          try { server.close(1000, "TCP closed"); } catch { /* noop */ }
        },
        abort() {
          try { server.close(1011, "TCP aborted"); } catch { /* noop */ }
        },
      })
    ).then(() => {
      console.log(`[relay] pipe done for ${target}, ${totalBytes} bytes`);
    }).catch((e) => {
      console.error(`[relay] pipe error for ${target}: ${e}`);
      try { server.close(1011, "pipe error"); } catch { /* noop */ }
    }).finally(async () => {
      if (totalBytes > 0) {
        await checkAndUpdateQuota(env.HYDRA_QUOTAS, deviceId, totalBytes, defaultLimit);
      }
    });

    ctx.waitUntil(tcpToWsTask);

    return new Response(null, { status: 101, webSocket: client });
  },
};
