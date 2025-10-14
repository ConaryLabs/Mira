import { useEffect, useRef, useCallback } from "react";

// Minimal, robust WS hook that:
// - Routes assistant_final, artifact_created, document_list, tool_writes
// - ACKs assistant_final so backend can retry on drop
// - Replays missed messages queued in backend on reconnect (requests history)
// - Emits concise console logs so you can see routing in devtools

export type WsMessage =
  | { type: "assistant_final"; id: number; content: string; artifacts?: ArtifactSummary[] }
  | { type: "artifact_created"; artifacts: ArtifactSummary[] }
  | { type: "document_list"; documents: DocumentSummary[] }
  | { type: "tool_writes"; files: string[] }
  | { type: "missed_messages"; items: WsMessage[] }
  | { type: "pong" }
  | { type: string; [key: string]: any };

export type ArtifactSummary = {
  title: string;
  path?: string | null;
  language: string;
  content_preview?: string;
};

export type DocumentSummary = {
  id: string;
  title: string;
  path?: string | null;
  updated_at?: string;
};

export type UseWebSocketHandlers = {
  onAssistantFinal?: (msg: Extract<WsMessage, { type: "assistant_final" }>) => void;
  onArtifactCreated?: (msg: Extract<WsMessage, { type: "artifact_created" }>) => void;
  onDocumentList?: (msg: Extract<WsMessage, { type: "document_list" }>) => void;
  onToolWrites?: (msg: Extract<WsMessage, { type: "tool_writes" }>) => void;
  onRaw?: (msg: WsMessage) => void;
};

export function useWebSocket(
  url: string,
  handlers: UseWebSocketHandlers,
) {
  const wsRef = useRef<WebSocket | null>(null);
  const heartbeat = useRef<NodeJS.Timeout | null>(null);
  const lastAckedId = useRef<number | null>(null);

  const send = useCallback((data: unknown) => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(data));
    }
  }, []);

  const ackAssistantFinal = useCallback((id: number) => {
    lastAckedId.current = id;
    send({ type: "assistant_final_ack", id });
  }, [send]);

  const requestHistory = useCallback(() => {
    send({ type: "chat.get_recent" });
    send({ type: "artifacts.get_recent" });
    send({ type: "documents.get_list" });
  }, [send]);

  useEffect(() => {
    if (wsRef.current && (wsRef.current.readyState === WebSocket.OPEN || wsRef.current.readyState === WebSocket.CONNECTING)) {
      return;
    }

    const ws = new WebSocket(url);
    wsRef.current = ws;

    ws.onopen = () => {
      console.info("WS connected", url);
      requestHistory();
      // start heartbeat
      heartbeat.current = setInterval(() => {
        send({ type: "ping" });
      }, 15000);
    };

    ws.onmessage = (ev) => {
      let msg: WsMessage;
      try {
        msg = JSON.parse(ev.data);
      } catch (e) {
        console.warn("WS non-JSON message", ev.data);
        return;
      }

      handlers.onRaw?.(msg);

      switch (msg.type) {
        case "assistant_final": {
          console.debug("WS route: assistant_final", { id: msg.id, text_len: msg.content?.length ?? 0 });
          handlers.onAssistantFinal?.(msg);
          if (typeof msg.id === "number") ackAssistantFinal(msg.id);
          break;
        }
        case "artifact_created": {
          console.debug("WS route: artifact_created", { count: msg.artifacts?.length ?? 0 });
          handlers.onArtifactCreated?.(msg);
          break;
        }
        case "document_list": {
          console.debug("WS route: document_list", { count: msg.documents?.length ?? 0 });
          handlers.onDocumentList?.(msg);
          break;
        }
        case "tool_writes": {
          console.debug("WS route: tool_writes", { files: msg.files });
          handlers.onToolWrites?.(msg);
          break;
        }
        case "missed_messages": {
          console.info("WS route: missed_messages", { count: msg.items?.length ?? 0 });
          // Replay each via the same router so UI updates as if they arrived live
          if (Array.isArray(msg.items)) {
            msg.items.forEach((it) => {
              // naive replay through the same handler
              if (it && typeof it === "object") {
                handlers.onRaw?.(it as WsMessage);
                if ((it as any).type === "assistant_final") {
                  handlers.onAssistantFinal?.(it as any);
                } else if ((it as any).type === "artifact_created") {
                  handlers.onArtifactCreated?.(it as any);
                } else if ((it as any).type === "document_list") {
                  handlers.onDocumentList?.(it as any);
                } else if ((it as any).type === "tool_writes") {
                  handlers.onToolWrites?.(it as any);
                }
              }
            });
          }
          break;
        }
        case "pong": {
          break;
        }
        default: {
          console.warn("WS unhandled message type", msg.type, msg);
          break;
        }
      }
    };

    ws.onerror = (ev) => {
      console.error("WS error", ev);
    };

    ws.onclose = () => {
      console.info("WS closed, retrying in 1s");
      if (heartbeat.current) clearInterval(heartbeat.current);
      heartbeat.current = null;
      setTimeout(() => {
        wsRef.current = null;
        // Reconnect triggers useEffect again
        useWebSocket(url, handlers);
      }, 1000);
    };

    return () => {
      if (heartbeat.current) clearInterval(heartbeat.current);
      ws.close();
    };
  }, [url, handlers, requestHistory, ackAssistantFinal, send]);

  return { send };
}
