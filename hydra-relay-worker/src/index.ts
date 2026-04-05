import { connect } from "cloudflare:sockets";
import { getAddress, keccak256, recoverMessageAddress, toBytes } from "viem";

interface Env {
  HYDRA_QUOTAS: KVNamespace;
  HYDRA_DEALER_PROFILES: KVNamespace;
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

interface DealerProfileRecord {
  address: string;
  display_name: string;
  contact_handle: string;
  instructions_by_method: Record<string, string>;
  general_notes: string;
  updated_at: number;
}

const DEALER_PROFILE_TIMESTAMP_TOLERANCE_MS = 5 * 60 * 1000;

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

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function dealerProfileKey(address: string): string {
  return `dealer-profile:${address.toLowerCase()}`;
}

function canonicalAddress(address: string): string {
  return getAddress(address).toLowerCase();
}

function dealerProfileMessage(
  address: string,
  timestampMs: number,
  path: string,
  body: string,
): string {
  const bodyHash = keccak256(toBytes(body));
  return [
    "Hydra Dealer Profile Update",
    `Address: ${address}`,
    `Timestamp: ${timestampMs}`,
    "Method: PUT",
    `Path: ${path}`,
    `Body-Keccak256: ${bodyHash}`,
  ].join("\n");
}

function normalizeDealerProfilePayload(raw: unknown, address: string): DealerProfileRecord {
  const input = typeof raw === "object" && raw !== null ? raw as Record<string, unknown> : {};
  const instructionsInput =
    typeof input.instructions_by_method === "object" && input.instructions_by_method !== null
      ? input.instructions_by_method as Record<string, unknown>
      : {};
  const instructionsByMethod = Object.fromEntries(
    Object.entries(instructionsInput)
      .map(([key, value]) => [key.trim().toLowerCase(), String(value ?? "").trim()])
      .filter(([key, value]) => key.length > 0 && value.length > 0),
  );

  return {
    address,
    display_name: String(input.display_name ?? "").trim(),
    contact_handle: String(input.contact_handle ?? "").trim(),
    instructions_by_method: instructionsByMethod,
    general_notes: String(input.general_notes ?? "").trim(),
    updated_at: Date.now(),
  };
}

async function handleDealerProfileGet(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);
  const addressPart = url.pathname.split("/").pop();
  if (!addressPart) {
    return jsonResponse({ error: "Missing dealer address." }, 400);
  }

  let address: string;
  try {
    address = canonicalAddress(addressPart);
  } catch {
    return jsonResponse({ error: "Invalid dealer address." }, 400);
  }

  const raw = await env.HYDRA_DEALER_PROFILES.get(dealerProfileKey(address));
  if (!raw) {
    return jsonResponse({ error: "Dealer profile not found." }, 404);
  }

  return new Response(raw, {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

async function handleDealerProfilePut(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);
  const addressPart = url.pathname.split("/").pop();
  if (!addressPart) {
    return jsonResponse({ error: "Missing dealer address." }, 400);
  }

  const rawBody = await request.text();
  const addressHeader = request.headers.get("X-Hydra-Address");
  const signature = request.headers.get("X-Hydra-Signature");
  const timestampHeader = request.headers.get("X-Hydra-Timestamp");
  if (!addressHeader || !signature || !timestampHeader) {
    return jsonResponse(
      { error: "Missing X-Hydra-Address, X-Hydra-Signature, or X-Hydra-Timestamp." },
      401,
    );
  }

  let pathAddress: string;
  let headerAddress: string;
  try {
    pathAddress = canonicalAddress(addressPart);
    headerAddress = canonicalAddress(addressHeader);
  } catch {
    return jsonResponse({ error: "Invalid dealer address." }, 400);
  }
  if (pathAddress !== headerAddress) {
    return jsonResponse({ error: "Dealer address mismatch." }, 401);
  }

  const timestampMs = Number(timestampHeader);
  if (!Number.isFinite(timestampMs)) {
    return jsonResponse({ error: "Invalid signature timestamp." }, 401);
  }
  if (Math.abs(Date.now() - timestampMs) > DEALER_PROFILE_TIMESTAMP_TOLERANCE_MS) {
    return jsonResponse({ error: "Signature timestamp expired. Refresh and try again." }, 401);
  }

  const message = dealerProfileMessage(headerAddress, timestampMs, url.pathname, rawBody);
  let recovered: string;
  try {
    recovered = canonicalAddress(
      await recoverMessageAddress({ message, signature: signature as `0x${string}` }),
    );
  } catch {
    return jsonResponse({ error: "Invalid dealer profile signature." }, 401);
  }
  if (recovered !== headerAddress) {
    return jsonResponse({ error: "Dealer profile signature address mismatch." }, 401);
  }

  let parsedBody: unknown;
  try {
    parsedBody = JSON.parse(rawBody);
  } catch {
    return jsonResponse({ error: "Dealer profile body must be valid JSON." }, 400);
  }

  const normalized = normalizeDealerProfilePayload(parsedBody, headerAddress);
  await env.HYDRA_DEALER_PROFILES.put(
    dealerProfileKey(headerAddress),
    JSON.stringify(normalized),
  );
  return jsonResponse(normalized, 200);
}

function hexPreview(data: Uint8Array, maxBytes = 32): string {
  return Array.from(data.slice(0, maxBytes))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

function parseSocketTarget(target: string): { hostname: string; port: number } {
  if (target.startsWith("[")) {
    const end = target.indexOf("]");
    if (end === -1) {
      throw new Error(`Invalid IPv6 target: ${target}`);
    }

    const hostname = target.slice(1, end);
    const rest = target.slice(end + 1);
    const port = rest.startsWith(":") ? parseInt(rest.slice(1) || "443", 10) : 443;
    if (!hostname || Number.isNaN(port)) {
      throw new Error(`Invalid IPv6 target: ${target}`);
    }
    return { hostname, port };
  }

  const lastColon = target.lastIndexOf(":");
  if (lastColon === -1) {
    return { hostname: target, port: 443 };
  }

  const hostname = target.slice(0, lastColon);
  const port = parseInt(target.slice(lastColon + 1) || "443", 10);
  if (!hostname || Number.isNaN(port)) {
    throw new Error(`Invalid target: ${target}`);
  }

  return { hostname, port };
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
  const startedAt = Date.now();
  const traceId = crypto.randomUUID().slice(0, 8);
  let tcpSocket: Socket;
  try {
    const { hostname, port } = parseSocketTarget(target);
    console.log(`[relay:${traceId}] connect start target=${target} device=${deviceId}`);
    tcpSocket = connect(
      { hostname, port },
      { secureTransport: "off", allowHalfOpen: true },
    );
  } catch (error) {
    console.error(`[relay:${traceId}] connect setup failed target=${target}: ${String(error)}`);
    server.close(1011, `TCP connect failed: ${String(error)}`);
    return new Response(null, { status: 101, webSocket: client });
  }

  void tcpSocket.opened
    .then((info) => {
      console.log(
        `[relay:${traceId}] tcp opened target=${target} remote=${info.remoteAddress ?? "unknown"} local=${info.localAddress ?? "unknown"}`,
      );
    })
    .catch((error) => {
      console.error(`[relay:${traceId}] tcp open failed target=${target}: ${String(error)}`);
      try {
        server.close(1011, "TCP open failed");
      } catch {
        // noop
      }
    });

  void tcpSocket.closed
    .then(() => {
      console.log(
        `[relay:${traceId}] tcp closed target=${target} bytes=${totalBytes} duration_ms=${Date.now() - startedAt}`,
      );
      try {
        server.close(1000, "TCP closed");
      } catch {
        // noop
      }
    })
    .catch((error) => {
      console.error(`[relay:${traceId}] tcp closed with error target=${target}: ${String(error)}`);
      try {
        server.close(1011, "TCP closed with error");
      } catch {
        // noop
      }
    });

  const writer = tcpSocket.writable.getWriter();
  let clientToTcpFrames = 0;
  let tcpToClientFrames = 0;

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
    clientToTcpFrames += 1;
    if (clientToTcpFrames === 1) {
      console.log(
        `[relay:${traceId}] first client frame target=${target} bytes=${bytes.byteLength} hex=${hexPreview(bytes)}`,
      );
    }
    writer.write(bytes).catch(() => {
      console.error(
        `[relay:${traceId}] tcp write failed target=${target} frames=${clientToTcpFrames}`,
      );
      try {
        server.close(1011, "TCP write failed");
      } catch {
        // noop
      }
    });
  });

  server.addEventListener("close", () => {
    console.log(
      `[relay:${traceId}] websocket closed target=${target} bytes=${totalBytes} duration_ms=${Date.now() - startedAt}`,
    );
    writer.close().catch(() => {});
  });

  server.addEventListener("error", () => {
    console.error(`[relay:${traceId}] websocket error target=${target}`);
    writer.close().catch(() => {});
  });

  const task = tcpSocket.readable
    .pipeTo(
      new WritableStream({
        write(chunk: Uint8Array) {
          totalBytes += chunk.byteLength;
          tcpToClientFrames += 1;
          if (tcpToClientFrames === 1) {
            console.log(
              `[relay:${traceId}] first tcp frame target=${target} bytes=${chunk.byteLength} hex=${hexPreview(chunk)}`,
            );
          }
          server.send(chunk);
        },
        close() {
          console.log(
            `[relay:${traceId}] tcp readable closed target=${target} frames=${tcpToClientFrames}`,
          );
        },
        abort() {
          console.error(`[relay:${traceId}] tcp readable aborted target=${target}`);
          try {
            server.close(1011, "TCP aborted");
          } catch {
            // noop
          }
        },
      }),
    )
    .catch((error) => {
      console.error(`[relay:${traceId}] pipe error target=${target}: ${String(error)}`);
      try {
        server.close(1011, "pipe error");
      } catch {
        // noop
      }
    })
    .finally(async () => {
      console.log(
        `[relay:${traceId}] finalize target=${target} bytes=${totalBytes} c2t_frames=${clientToTcpFrames} t2c_frames=${tcpToClientFrames}`,
      );
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

    if (url.pathname.startsWith("/api/dealer-profiles/")) {
      if (request.method === "GET") {
        return handleDealerProfileGet(request, env);
      }
      if (request.method === "PUT") {
        return handleDealerProfilePut(request, env);
      }
      return jsonResponse({ error: "Method not allowed." }, 405);
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
