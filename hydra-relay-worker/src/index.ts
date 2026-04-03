import { connect } from "cloudflare:sockets";

interface Env {
  HYDRA_QUOTAS: KVNamespace;
  HYDRA_PROVIDER_SESSIONS: DurableObjectNamespace;
  DEFAULT_DAILY_QUOTA: string;
}

type SocketFrame = ArrayBuffer | Uint8Array;

interface ProviderControlMessage {
  type: "registered" | "connect" | "ready" | "close" | "closed" | "error" | "ping" | "pong";
  target?: string;
  message?: string;
}

interface WebSocketAttachment {
  role: "provider" | "consumer";
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
      // Reset corrupted entries on next write.
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

function defaultQuota(env: Env): number {
  return parseInt(env.DEFAULT_DAILY_QUOTA, 10) || 52428800;
}

function socketByteLength(message: SocketFrame | string): number {
  if (typeof message === "string") {
    return new TextEncoder().encode(message).byteLength;
  }
  if (message instanceof Uint8Array) {
    return message.byteLength;
  }
  return message.byteLength;
}

function parseSocketTarget(target: string): { hostname: string; port: number } {
  const parts = target.split(":");
  return {
    hostname: parts[0],
    port: parseInt(parts[1] || "443", 10),
  };
}

function websocketPair(): [WebSocket, WebSocket] {
  const pair = new WebSocketPair();
  return Object.values(pair) as [WebSocket, WebSocket];
}

async function handleDirectConnection(
  request: Request,
  env: Env,
  ctx: ExecutionContext,
): Promise<Response> {
  const target = request.headers.get("X-Hydra-Target");
  if (!target) {
    return new Response("Missing X-Hydra-Target header (format: host:port)", { status: 400 });
  }

  const deviceId = request.headers.get("X-Hydra-Device") || "anonymous";
  const limit = defaultQuota(env);
  const { allowed } = await checkAndUpdateQuota(env.HYDRA_QUOTAS, deviceId, 0, limit);
  if (!allowed) {
    return new Response("Daily quota exceeded", { status: 429 });
  }

  const [client, server] = websocketPair();
  server.accept();

  let totalBytes = 0;
  let tcpSocket: Socket;
  try {
    const { hostname, port } = parseSocketTarget(target);
    tcpSocket = connect(
      { hostname, port },
      { secureTransport: "off", allowHalfOpen: false },
    );
  } catch (error) {
    server.close(1011, `TCP connect failed: ${String(error)}`);
    return new Response(null, { status: 101, webSocket: client });
  }

  const writer = tcpSocket.writable.getWriter();

  server.addEventListener("message", (event: MessageEvent) => {
    const data = event.data;
    let bytes: Uint8Array;
    if (data instanceof ArrayBuffer) {
      bytes = new Uint8Array(data);
    } else if (data instanceof Uint8Array) {
      bytes = data;
    } else if (typeof data === "string") {
      bytes = new TextEncoder().encode(data);
    } else {
      return;
    }
    totalBytes += bytes.byteLength;
    writer.write(bytes).catch(() => {
      try {
        server.close(1011, "TCP write failed");
      } catch {
        // noop
      }
    });
  });

  server.addEventListener("close", () => {
    writer.close().catch(() => {});
  });

  server.addEventListener("error", () => {
    writer.close().catch(() => {});
  });

  const task = tcpSocket.readable
    .pipeTo(
      new WritableStream({
        write(chunk: Uint8Array) {
          totalBytes += chunk.byteLength;
          server.send(chunk);
        },
        close() {
          try {
            server.close(1000, "TCP closed");
          } catch {
            // noop
          }
        },
        abort() {
          try {
            server.close(1011, "TCP aborted");
          } catch {
            // noop
          }
        },
      }),
    )
    .catch((error) => {
      console.error(`[relay] pipe error for ${target}: ${String(error)}`);
      try {
        server.close(1011, "pipe error");
      } catch {
        // noop
      }
    })
    .finally(async () => {
      if (totalBytes > 0) {
        await checkAndUpdateQuota(env.HYDRA_QUOTAS, deviceId, totalBytes, limit);
      }
    });

  ctx.waitUntil(task);
  return new Response(null, { status: 101, webSocket: client });
}

export class HydraProviderSession {
  private provider: WebSocket | null = null;
  private consumer: WebSocket | null = null;
  private providerReady = false;
  private consumerDeviceId = "anonymous";
  private consumerLimit = 52428800;
  private consumerBytes = 0;
  private consumerBuffer: SocketFrame[] = [];

  constructor(
    private readonly state: DurableObjectState,
    private readonly env: Env,
  ) {}

  async fetch(request: Request): Promise<Response> {
    const upgradeHeader = request.headers.get("Upgrade");
    if (!upgradeHeader || upgradeHeader.toLowerCase() !== "websocket") {
      return new Response("WebSocket upgrade required", { status: 426 });
    }

    const mode = request.headers.get("X-Hydra-Mode");
    if (mode === "provider") {
      return this.acceptProvider();
    }
    if (mode === "consumer") {
      return this.acceptConsumer(request);
    }
    return new Response("Unsupported session mode", { status: 400 });
  }

  webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): void {
    const attachment = (ws.deserializeAttachment() || null) as WebSocketAttachment | null;
    if (attachment?.role === "provider") {
      this.handleProviderMessage(message);
    } else if (attachment?.role === "consumer") {
      this.handleConsumerMessage(message);
    }
  }

  webSocketClose(ws: WebSocket): void {
    const attachment = (ws.deserializeAttachment() || null) as WebSocketAttachment | null;
    if (attachment?.role === "provider") {
      this.provider = null;
      this.providerReady = false;
      this.closeConsumer(1011, "Provider offline");
      this.resetConsumerState();
    } else if (attachment?.role === "consumer") {
      this.notifyProvider({ type: "close" });
      this.flushConsumerQuota();
      this.resetConsumerState();
    }
  }

  webSocketError(ws: WebSocket): void {
    this.webSocketClose(ws);
  }

  private acceptProvider(): Response {
    const [client, server] = websocketPair();
    server.serializeAttachment({ role: "provider" } satisfies WebSocketAttachment);
    this.state.acceptWebSocket(server);

    if (this.provider && this.provider !== server) {
      try {
        this.provider.close(1012, "Provider replaced");
      } catch {
        // noop
      }
    }

    this.provider = server;
    this.providerReady = false;
    server.send(JSON.stringify({ type: "registered" } satisfies ProviderControlMessage));
    return new Response(null, { status: 101, webSocket: client });
  }

  private async acceptConsumer(request: Request): Promise<Response> {
    if (!this.provider) {
      return new Response("Provider is offline", { status: 503 });
    }
    if (this.consumer) {
      return new Response("Provider is busy", { status: 409 });
    }

    const target = request.headers.get("X-Hydra-Target");
    if (!target) {
      return new Response("Missing X-Hydra-Target header", { status: 400 });
    }

    const deviceId = request.headers.get("X-Hydra-Device") || "anonymous";
    const limit = defaultQuota(this.env);
    const { allowed } = await checkAndUpdateQuota(this.env.HYDRA_QUOTAS, deviceId, 0, limit);
    if (!allowed) {
      return new Response("Daily quota exceeded", { status: 429 });
    }

    const [client, server] = websocketPair();
    server.serializeAttachment({ role: "consumer" } satisfies WebSocketAttachment);
    this.state.acceptWebSocket(server);
    this.consumer = server;
    this.consumerDeviceId = deviceId;
    this.consumerLimit = limit;
    this.consumerBytes = 0;
    this.consumerBuffer = [];
    this.providerReady = false;

    if (!this.notifyProvider({ type: "connect", target })) {
      this.closeConsumer(1011, "Provider registration failed");
      this.resetConsumerState();
      return new Response("Provider unavailable", { status: 503 });
    }

    return new Response(null, { status: 101, webSocket: client });
  }

  private handleProviderMessage(message: string | ArrayBuffer): void {
    if (typeof message === "string") {
      let payload: ProviderControlMessage;
      try {
        payload = JSON.parse(message) as ProviderControlMessage;
      } catch {
        return;
      }

      switch (payload.type) {
        case "ready":
          this.providerReady = true;
          this.flushBufferedConsumerFrames();
          this.consumer?.send(JSON.stringify({ type: "ready" } satisfies ProviderControlMessage));
          return;
        case "error":
          this.closeConsumer(1011, payload.message || "Provider error");
          this.flushConsumerQuota();
          this.resetConsumerState();
          return;
        case "closed":
          this.closeConsumer(1000, "Provider closed");
          this.flushConsumerQuota();
          this.resetConsumerState();
          return;
        case "ping":
          this.notifyProvider({ type: "pong" });
          return;
        default:
          return;
      }
    }

    if (!this.providerReady || !this.consumer) {
      return;
    }

    this.consumerBytes += socketByteLength(message);
    this.consumer.send(message);
  }

  private handleConsumerMessage(message: string | ArrayBuffer): void {
    if (typeof message === "string") {
      return;
    }

    this.consumerBytes += socketByteLength(message);
    if (this.provider && this.providerReady) {
      this.provider.send(message);
      return;
    }

    if (this.consumerBuffer.length >= 64) {
      this.closeConsumer(1013, "Provider setup timeout");
      this.flushConsumerQuota();
      this.resetConsumerState();
      return;
    }
    this.consumerBuffer.push(message);
  }

  private flushBufferedConsumerFrames(): void {
    if (!this.provider || !this.providerReady) {
      return;
    }
    for (const frame of this.consumerBuffer) {
      this.provider.send(frame);
    }
    this.consumerBuffer = [];
  }

  private notifyProvider(message: ProviderControlMessage): boolean {
    if (!this.provider) {
      return false;
    }
    try {
      this.provider.send(JSON.stringify(message));
      return true;
    } catch {
      return false;
    }
  }

  private closeConsumer(code: number, reason: string): void {
    if (!this.consumer) {
      return;
    }
    try {
      this.consumer.close(code, reason);
    } catch {
      // noop
    }
  }

  private flushConsumerQuota(): void {
    const bytes = this.consumerBytes;
    const deviceId = this.consumerDeviceId;
    const limit = this.consumerLimit;
    if (bytes <= 0) {
      return;
    }
    void checkAndUpdateQuota(this.env.HYDRA_QUOTAS, deviceId, bytes, limit);
  }

  private resetConsumerState(): void {
    this.consumer = null;
    this.consumerBytes = 0;
    this.consumerBuffer = [];
    this.providerReady = false;
  }
}

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname === "/health") {
      return new Response("ok", { status: 200 });
    }

    if (url.pathname === "/quota") {
      const deviceId = url.searchParams.get("device_id") || "anonymous";
      const limit = defaultQuota(env);
      const { remaining } = await checkAndUpdateQuota(env.HYDRA_QUOTAS, deviceId, 0, limit);
      return Response.json({ remaining, limit });
    }

    const upgradeHeader = request.headers.get("Upgrade");
    if (!upgradeHeader || upgradeHeader.toLowerCase() !== "websocket") {
      return new Response("Hydra Relay. Send WebSocket upgrade to connect.", {
        status: 426,
        headers: { "Content-Type": "text/plain" },
      });
    }

    const mode = request.headers.get("X-Hydra-Mode");
    const targetAgent =
      request.headers.get("X-Hydra-Target-Agent") ||
      request.headers.get("X-Hydra-Agent") ||
      url.searchParams.get("agent");

    if ((mode === "provider" || mode === "consumer" || targetAgent) && targetAgent) {
      const id = env.HYDRA_PROVIDER_SESSIONS.idFromName(targetAgent);
      const stub = env.HYDRA_PROVIDER_SESSIONS.get(id);
      return stub.fetch(request);
    }

    return handleDirectConnection(request, env, ctx);
  },
};
